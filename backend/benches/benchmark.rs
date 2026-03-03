//! Kiro Gateway Benchmark CLI
//!
//! Usage:
//!   cargo bench --features bench -- run --gateway-url http://localhost:8000 --api-key mykey
//!   cargo bench --features bench -- standalone -c 10,50,100 -d 30 -s

use clap::{Parser, Subcommand};
use kiro_gateway::bench::{
    BenchmarkConfig, BenchmarkReport, BenchmarkRunner, MockKiroServer, MockServerConfig,
};

#[derive(Parser)]
#[command(name = "gateway_benchmark")]
#[command(about = "Benchmark tool for Kiro Gateway")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run benchmark against an existing gateway
    Run {
        /// Gateway URL to benchmark
        #[arg(short = 'g', long, default_value = "http://localhost:8000")]
        gateway_url: String,

        /// API key for authentication
        #[arg(short = 'k', long, default_value = "benchmark-key")]
        api_key: String,

        /// Concurrency levels to test (comma-separated)
        #[arg(
            short = 'c',
            long,
            default_value = "10,50,100,200",
            value_delimiter = ','
        )]
        concurrency: Vec<usize>,

        /// Duration per concurrency level in seconds
        #[arg(short = 'd', long, default_value = "30")]
        duration: u64,

        /// Enable streaming mode
        #[arg(short = 's', long)]
        streaming: bool,

        /// API format (openai or anthropic)
        #[arg(short = 'f', long, default_value = "openai")]
        format: String,

        /// Model to request
        #[arg(short = 'm', long, default_value = "claude-sonnet-4-20250514")]
        model: String,

        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Skip warmup requests
        #[arg(long)]
        no_warmup: bool,
    },

    /// Run standalone benchmark (starts mock server + runs benchmark)
    Standalone {
        /// Concurrency levels to test (comma-separated)
        #[arg(short = 'c', long, default_value = "10,50,100", value_delimiter = ',')]
        concurrency: Vec<usize>,

        /// Duration per concurrency level in seconds
        #[arg(short = 'd', long, default_value = "10")]
        duration: u64,

        /// Enable streaming mode
        #[arg(short = 's', long)]
        streaming: bool,

        /// API format (openai or anthropic)
        #[arg(short = 'f', long, default_value = "openai")]
        format: String,

        /// Mock server chunk latency in milliseconds
        #[arg(long, default_value = "5")]
        chunk_latency: u64,

        /// Mock server chunk count
        #[arg(long, default_value = "20")]
        chunk_count: usize,

        /// Mock server error rate (0.0 to 1.0)
        #[arg(long, default_value = "0.0")]
        error_rate: f64,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            gateway_url,
            api_key,
            concurrency,
            duration,
            streaming,
            format,
            model,
            json,
            no_warmup,
        } => {
            let config = BenchmarkConfig {
                gateway_url,
                api_key,
                concurrency_levels: concurrency,
                duration_secs: duration,
                streaming,
                format: format.parse().unwrap_or_default(),
                model,
                warmup_requests: if no_warmup { 0 } else { 10 },
                ..Default::default()
            };

            run_benchmark(config, json).await?;
        }

        Commands::Standalone {
            concurrency,
            duration,
            streaming,
            format,
            chunk_latency,
            chunk_count,
            error_rate,
            json,
        } => {
            // Start mock server
            let mock_config = MockServerConfig {
                port: 0,
                chunk_latency_ms: chunk_latency,
                chunk_count,
                chunk_size: 50,
                error_rate,
                streaming,
            };

            let mut mock_server = MockKiroServer::new(mock_config);
            let mock_port = mock_server.start().await?;
            println!("Mock Kiro server started on port {}", mock_port);

            // Note: In standalone mode, we benchmark directly against the mock server
            // since we don't have a gateway running. For full gateway testing,
            // use the 'run' command with a running gateway.
            let config = BenchmarkConfig {
                gateway_url: format!("http://127.0.0.1:{}", mock_port),
                api_key: "benchmark-key".to_string(),
                concurrency_levels: concurrency,
                duration_secs: duration,
                streaming,
                format: format.parse().unwrap_or_default(),
                warmup_requests: 5,
                ..Default::default()
            };

            // Override endpoint for mock server
            run_benchmark_mock(config, json).await?;

            mock_server.stop();
        }
    }

    Ok(())
}

async fn run_benchmark(config: BenchmarkConfig, json_output: bool) -> anyhow::Result<()> {
    println!("Starting benchmark against {}", config.gateway_url);
    println!("Format: {}, Streaming: {}", config.format, config.streaming);
    println!(
        "Concurrency levels: {:?}, Duration: {}s each",
        config.concurrency_levels, config.duration_secs
    );
    println!();

    let runner = BenchmarkRunner::new(config.clone());

    // Warmup
    if config.warmup_requests > 0 {
        if let Err(e) = runner.warmup().await {
            eprintln!("Warning: Warmup failed: {}", e);
            eprintln!("Make sure the gateway is running and accessible.");
            return Err(e);
        }
    }

    // Run benchmark
    let results = runner.run().await;

    // Generate report
    let report = BenchmarkReport::from_results(results);

    if json_output {
        println!("{}", report.to_json());
    } else {
        report.print_table();
    }

    Ok(())
}

async fn run_benchmark_mock(config: BenchmarkConfig, json_output: bool) -> anyhow::Result<()> {
    println!("Starting standalone benchmark against mock server");
    println!("Format: {}, Streaming: {}", config.format, config.streaming);
    println!(
        "Concurrency levels: {:?}, Duration: {}s each",
        config.concurrency_levels, config.duration_secs
    );
    println!();

    // Create a custom runner that hits the mock endpoint directly
    let runner = MockBenchmarkRunner::new(config.clone());

    // Warmup
    if config.warmup_requests > 0 {
        runner.warmup().await?;
    }

    // Run benchmark
    let results = runner.run().await;

    // Generate report
    let report = BenchmarkReport::from_results(results);

    if json_output {
        println!("{}", report.to_json());
    } else {
        report.print_table();
    }

    Ok(())
}

/// Benchmark runner for mock server (hits /generateAssistantResponse directly)
struct MockBenchmarkRunner {
    config: BenchmarkConfig,
    client: reqwest::Client,
}

impl MockBenchmarkRunner {
    fn new(config: BenchmarkConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .pool_max_idle_per_host(500)
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }

    async fn warmup(&self) -> anyhow::Result<()> {
        println!("Running {} warmup requests...", self.config.warmup_requests);

        for _ in 0..self.config.warmup_requests {
            let _ = self.execute_request().await;
        }

        Ok(())
    }

    async fn run(&self) -> Vec<(usize, kiro_gateway::bench::metrics::MetricsSnapshot)> {
        use kiro_gateway::bench::MetricsCollector;
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        use sysinfo::System;
        use tokio::sync::Semaphore;

        let mut results = Vec::new();

        for &concurrency in &self.config.concurrency_levels {
            println!("\nRunning benchmark at concurrency {}...", concurrency);

            let metrics = Arc::new(MetricsCollector::new());
            let semaphore = Arc::new(Semaphore::new(concurrency));
            let duration = Duration::from_secs(self.config.duration_secs);

            metrics.start();
            let start = Instant::now();

            // Spawn resource sampling task
            let metrics_for_sampling = metrics.clone();
            let sampling_duration = duration;
            let sampling_handle = tokio::spawn(async move {
                let mut sys = System::new_all();
                while start.elapsed() < sampling_duration {
                    metrics_for_sampling.sample_resources(&mut sys);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            });

            let mut handles = Vec::new();

            while start.elapsed() < duration {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let client = self.client.clone();
                let url = format!("{}/generateAssistantResponse", self.config.gateway_url);
                let metrics = metrics.clone();

                let handle = tokio::spawn(async move {
                    let req_start = Instant::now();
                    let result = client
                        .post(&url)
                        .json(&serde_json::json!({"test": true}))
                        .send()
                        .await;

                    match result {
                        Ok(resp) if resp.status().is_success() => {
                            let ttfb = req_start.elapsed();
                            let bytes = resp.bytes().await.map(|b| b.len() as u64).unwrap_or(0);
                            let latency = req_start.elapsed();
                            metrics.record_success(latency, Some(ttfb), bytes);
                        }
                        _ => {
                            metrics.record_error();
                        }
                    }
                    drop(permit);
                });

                handles.push(handle);
                tokio::time::sleep(Duration::from_micros(100)).await;
            }

            for handle in handles {
                let _ = handle.await;
            }

            // Wait for sampling to finish
            let _ = sampling_handle.await;

            metrics.stop();
            let snapshot = metrics.snapshot();

            println!(
                "  RPS: {:.1}, p50: {:.1}ms, p99: {:.1}ms, success: {:.1}%, CPU: {:.1}%, Mem: {:.0}MB",
                snapshot.requests_per_second,
                snapshot.latency_p50,
                snapshot.latency_p99,
                snapshot.success_rate,
                snapshot.avg_cpu,
                snapshot.avg_memory_mb
            );

            results.push((concurrency, snapshot));
        }

        results
    }

    async fn execute_request(&self) -> anyhow::Result<()> {
        let url = format!("{}/generateAssistantResponse", self.config.gateway_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"test": true}))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Request failed");
        }

        let _ = resp.bytes().await?;
        Ok(())
    }
}
