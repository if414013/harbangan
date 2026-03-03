//! Benchmark runner with concurrency control.

use futures::StreamExt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use super::config::{ApiFormat, BenchmarkConfig};
use super::metrics::{MetricsCollector, MetricsSnapshot};

/// Benchmark runner that executes requests against the gateway
pub struct BenchmarkRunner {
    config: BenchmarkConfig,
    client: reqwest::Client,
}

impl BenchmarkRunner {
    /// Create a new benchmark runner
    pub fn new(config: BenchmarkConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .pool_max_idle_per_host(500)
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    /// Run warmup requests
    pub async fn warmup(&self) -> anyhow::Result<()> {
        println!("Running {} warmup requests...", self.config.warmup_requests);

        for _ in 0..self.config.warmup_requests {
            let _ = self.execute_request().await;
        }

        Ok(())
    }

    /// Run benchmark at a specific concurrency level
    pub async fn run_at_concurrency(&self, concurrency: usize) -> MetricsSnapshot {
        let metrics = Arc::new(MetricsCollector::new());
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let duration = Duration::from_secs(self.config.duration_secs);

        metrics.start();
        let start = Instant::now();

        // Spawn tasks until duration expires
        let mut handles = Vec::new();

        while start.elapsed() < duration {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let client = self.client.clone();
            let config = self.config.clone();
            let metrics = metrics.clone();

            let handle = tokio::spawn(async move {
                let result = execute_single_request(&client, &config).await;
                match result {
                    Ok((latency, ttfb, bytes)) => {
                        metrics.record_success(latency, ttfb, bytes);
                    }
                    Err(_) => {
                        metrics.record_error();
                    }
                }
                drop(permit);
            });

            handles.push(handle);

            // Small delay to prevent overwhelming the system
            tokio::time::sleep(Duration::from_micros(100)).await;
        }

        // Wait for all in-flight requests to complete
        for handle in handles {
            let _ = handle.await;
        }

        metrics.stop();
        metrics.snapshot()
    }

    /// Run the full benchmark across all concurrency levels
    pub async fn run(&self) -> Vec<(usize, MetricsSnapshot)> {
        let mut results = Vec::new();

        for &concurrency in &self.config.concurrency_levels {
            println!("\nRunning benchmark at concurrency {}...", concurrency);
            let snapshot = self.run_at_concurrency(concurrency).await;
            println!(
                "  RPS: {:.1}, p50: {:.1}ms, p99: {:.1}ms, success: {:.1}%",
                snapshot.requests_per_second,
                snapshot.latency_p50,
                snapshot.latency_p99,
                snapshot.success_rate
            );
            results.push((concurrency, snapshot));
        }

        results
    }

    /// Execute a single request (for warmup)
    async fn execute_request(&self) -> anyhow::Result<()> {
        let _ = execute_single_request(&self.client, &self.config).await?;
        Ok(())
    }
}

/// Execute a single request and return (latency, ttfb, bytes)
async fn execute_single_request(
    client: &reqwest::Client,
    config: &BenchmarkConfig,
) -> anyhow::Result<(Duration, Option<Duration>, u64)> {
    let url = format!("{}{}", config.gateway_url, config.endpoint_path());
    let body = build_request_body(config);

    let start = Instant::now();

    let mut request = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", config.api_key));

    // Add format-specific headers
    match config.format {
        ApiFormat::Anthropic => {
            request = request
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", "2023-06-01");
        }
        ApiFormat::OpenAI => {}
    }

    let response = request.json(&body).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Request failed with status: {}", response.status());
    }

    let ttfb = start.elapsed();
    let mut total_bytes = 0u64;

    if config.streaming {
        // Consume the stream
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => total_bytes += bytes.len() as u64,
                Err(e) => anyhow::bail!("Stream error: {}", e),
            }
        }
    } else {
        let bytes = response.bytes().await?;
        total_bytes = bytes.len() as u64;
    }

    let latency = start.elapsed();
    Ok((latency, Some(ttfb), total_bytes))
}

/// Build the request body based on API format
fn build_request_body(config: &BenchmarkConfig) -> serde_json::Value {
    match config.format {
        ApiFormat::OpenAI => {
            serde_json::json!({
                "model": config.model,
                "messages": [
                    {"role": "user", "content": "Say hello in exactly 10 words."}
                ],
                "stream": config.streaming,
                "max_tokens": 100
            })
        }
        ApiFormat::Anthropic => {
            serde_json::json!({
                "model": config.model,
                "messages": [
                    {"role": "user", "content": "Say hello in exactly 10 words."}
                ],
                "stream": config.streaming,
                "max_tokens": 100
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_openai_request() {
        let config = BenchmarkConfig {
            format: ApiFormat::OpenAI,
            model: "test-model".to_string(),
            streaming: true,
            ..Default::default()
        };

        let body = build_request_body(&config);
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], true);
        assert!(body["messages"].is_array());
    }

    #[test]
    fn test_build_anthropic_request() {
        let config = BenchmarkConfig {
            format: ApiFormat::Anthropic,
            model: "test-model".to_string(),
            streaming: false,
            ..Default::default()
        };

        let body = build_request_body(&config);
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], false);
    }
}
