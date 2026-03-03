use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

/// Ring buffer capacity for samples (15 minutes at ~4 samples/sec)
const RING_BUFFER_CAPACITY: usize = 3600;

/// Per-model statistics
#[derive(Debug)]
pub struct ModelStats {
    pub request_count: AtomicU64,
    pub total_latency_ms: AtomicU64,
    pub total_input_tokens: AtomicU64,
    pub total_output_tokens: AtomicU64,
}

impl ModelStats {
    /// Create new model statistics
    pub fn new() -> Self {
        Self {
            request_count: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            total_input_tokens: AtomicU64::new(0),
            total_output_tokens: AtomicU64::new(0),
        }
    }

    /// Record a request for this model
    pub fn record_request(&self, latency_ms: f64, input_tokens: u64, output_tokens: u64) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency_ms as u64, Ordering::Relaxed);
        self.total_input_tokens
            .fetch_add(input_tokens, Ordering::Relaxed);
        self.total_output_tokens
            .fetch_add(output_tokens, Ordering::Relaxed);
    }
}

impl Default for ModelStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracker for streaming requests that updates output tokens asynchronously.
///
/// This tracker holds an atomic counter that can be updated by the stream processing
/// logic as usage events arrive. When the tracker is dropped (stream ends), it
/// automatically records the final metrics with the accumulated output token count.
pub struct StreamingMetricsTracker {
    metrics: Arc<MetricsCollector>,
    model: String,
    input_tokens: u64,
    output_tokens: Arc<AtomicU64>,
    start_time: Instant,
    completed: bool,
}

impl StreamingMetricsTracker {
    pub fn new(metrics: Arc<MetricsCollector>, model: String, input_tokens: u64) -> Self {
        metrics.record_request_start();
        Self {
            metrics,
            model,
            input_tokens,
            output_tokens: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
            completed: false,
        }
    }

    pub fn output_tokens_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.output_tokens)
    }

    pub fn complete(&mut self) {
        if !self.completed {
            let latency_ms = self.start_time.elapsed().as_secs_f64() * 1000.0;
            let output = self.output_tokens.load(Ordering::Relaxed);
            self.metrics
                .record_request_end(latency_ms, &self.model, self.input_tokens, output);
            self.completed = true;
        }
    }
}

impl Drop for StreamingMetricsTracker {
    fn drop(&mut self) {
        self.complete();
    }
}

/// Metrics collector for monitoring dashboard
pub struct MetricsCollector {
    /// Current active connections/requests
    pub active_connections: AtomicU64,

    /// Lifetime total request count
    total_requests: AtomicU64,

    /// Lifetime total error count
    total_errors: AtomicU64,

    /// Errors keyed by type
    errors_by_type: DashMap<String, AtomicU64>,

    /// Latency samples (time, latency_ms) - ring buffer
    latency_samples: Mutex<VecDeque<(Instant, f64)>>,

    /// Request rate samples (time, count) - ring buffer
    request_rate_samples: Mutex<VecDeque<(Instant, u64)>>,

    /// Token counts (time, input_tokens, output_tokens) - ring buffer
    token_counts: Mutex<VecDeque<(Instant, u64, u64)>>,

    /// Per-model statistics
    per_model_stats: DashMap<String, ModelStats>,

    /// Per-user per-model statistics
    per_user_stats: DashMap<Uuid, DashMap<String, ModelStats>>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            active_connections: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            errors_by_type: DashMap::new(),
            latency_samples: Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAPACITY)),
            request_rate_samples: Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAPACITY)),
            token_counts: Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAPACITY)),
            per_model_stats: DashMap::new(),
            per_user_stats: DashMap::new(),
        }
    }

    /// Record the start of a request
    pub fn record_request_start(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the end of a request
    pub fn record_request_end(
        &self,
        latency_ms: f64,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);

        let now = Instant::now();

        if let Ok(mut samples) = self.latency_samples.lock() {
            if samples.len() >= RING_BUFFER_CAPACITY {
                samples.pop_front();
            }
            samples.push_back((now, latency_ms));
        }

        if let Ok(mut counts) = self.token_counts.lock() {
            if counts.len() >= RING_BUFFER_CAPACITY {
                counts.pop_front();
            }
            counts.push_back((now, input_tokens, output_tokens));
        }

        if let Ok(mut samples) = self.request_rate_samples.lock() {
            if samples.len() >= RING_BUFFER_CAPACITY {
                samples.pop_front();
            }
            samples.push_back((now, 1));
        }

        self.per_model_stats
            .entry(model.to_string())
            .or_default()
            .record_request(latency_ms, input_tokens, output_tokens);
    }

    /// Record the end of a request with per-user tracking.
    #[allow(dead_code)]
    pub fn record_request_end_for_user(
        &self,
        latency_ms: f64,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        user_id: Uuid,
    ) {
        // Record global stats
        self.record_request_end(latency_ms, model, input_tokens, output_tokens);

        // Record per-user stats
        let user_models = self.per_user_stats.entry(user_id).or_default();
        user_models
            .entry(model.to_string())
            .or_default()
            .record_request(latency_ms, input_tokens, output_tokens);
    }

    /// Get per-user statistics.
    #[allow(dead_code)]
    pub fn get_user_stats(&self, user_id: Uuid) -> Vec<(String, ModelStats)> {
        self.per_user_stats
            .get(&user_id)
            .map(|user_models| {
                user_models
                    .iter()
                    .map(|entry| {
                        let model = entry.key().clone();
                        let stats = entry.value();
                        let cloned = ModelStats {
                            request_count: AtomicU64::new(
                                stats.request_count.load(Ordering::Relaxed),
                            ),
                            total_latency_ms: AtomicU64::new(
                                stats.total_latency_ms.load(Ordering::Relaxed),
                            ),
                            total_input_tokens: AtomicU64::new(
                                stats.total_input_tokens.load(Ordering::Relaxed),
                            ),
                            total_output_tokens: AtomicU64::new(
                                stats.total_output_tokens.load(Ordering::Relaxed),
                            ),
                        };
                        (model, cloned)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Record an error
    pub fn record_error(&self, error_type: &str) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);

        self.errors_by_type
            .entry(error_type.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Get current active connections
    pub fn get_active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get latency percentiles (p50, p95, p99)
    pub fn get_latency_percentiles(&self) -> (f64, f64, f64) {
        let samples = match self.latency_samples.lock() {
            Ok(s) => s,
            Err(_) => return (0.0, 0.0, 0.0),
        };

        if samples.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let mut latencies: Vec<f64> = samples.iter().map(|(_, lat)| *lat).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let len = latencies.len();
        let p50_idx = (len as f64 * 0.50) as usize;
        let p95_idx = (len as f64 * 0.95) as usize;
        let p99_idx = (len as f64 * 0.99) as usize;

        let p50 = latencies.get(p50_idx.min(len - 1)).copied().unwrap_or(0.0);
        let p95 = latencies.get(p95_idx.min(len - 1)).copied().unwrap_or(0.0);
        let p99 = latencies.get(p99_idx.min(len - 1)).copied().unwrap_or(0.0);

        (p50, p95, p99)
    }

    /// Get per-model statistics
    pub fn get_model_stats(&self) -> Vec<(String, ModelStats)> {
        self.per_model_stats
            .iter()
            .map(|entry| {
                let model = entry.key().clone();
                let stats = entry.value();
                let cloned_stats = ModelStats {
                    request_count: AtomicU64::new(stats.request_count.load(Ordering::Relaxed)),
                    total_latency_ms: AtomicU64::new(
                        stats.total_latency_ms.load(Ordering::Relaxed),
                    ),
                    total_input_tokens: AtomicU64::new(
                        stats.total_input_tokens.load(Ordering::Relaxed),
                    ),
                    total_output_tokens: AtomicU64::new(
                        stats.total_output_tokens.load(Ordering::Relaxed),
                    ),
                };
                (model, cloned_stats)
            })
            .collect()
    }

    /// Get a JSON snapshot of all metrics for the web UI
    pub fn to_json_snapshot(&self) -> serde_json::Value {
        let (p50, p95, p99) = self.get_latency_percentiles();

        let model_stats: Vec<serde_json::Value> = self
            .get_model_stats()
            .iter()
            .map(|(name, stats)| {
                let requests = stats.request_count.load(Ordering::Relaxed);
                let total_latency = stats.total_latency_ms.load(Ordering::Relaxed);
                let avg_latency = if requests > 0 {
                    total_latency as f64 / requests as f64
                } else {
                    0.0
                };
                serde_json::json!({
                    "name": name,
                    "requests": requests,
                    "avg_latency_ms": avg_latency,
                    "input_tokens": stats.total_input_tokens.load(Ordering::Relaxed),
                    "output_tokens": stats.total_output_tokens.load(Ordering::Relaxed),
                })
            })
            .collect();

        let errors: std::collections::HashMap<String, u64> = self
            .errors_by_type
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().load(Ordering::Relaxed)))
            .collect();

        serde_json::json!({
            "active_connections": self.get_active_connections(),
            "total_requests": self.total_requests.load(Ordering::Relaxed),
            "total_errors": self.total_errors.load(Ordering::Relaxed),
            "latency": { "p50": p50, "p95": p95, "p99": p99 },
            "models": model_stats,
            "errors_by_type": errors,
        })
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector_new() {
        let collector = MetricsCollector::new();
        assert_eq!(collector.get_active_connections(), 0);
    }

    #[test]
    fn test_record_request_lifecycle() {
        let collector = MetricsCollector::new();

        collector.record_request_start();
        assert_eq!(collector.get_active_connections(), 1);

        collector.record_request_end(150.5, "claude-sonnet-4", 100, 200);
        assert_eq!(collector.get_active_connections(), 0);
    }

    #[test]
    fn test_latency_percentiles() {
        let collector = MetricsCollector::new();

        for i in 1..=100 {
            collector.record_request_end(i as f64, "test-model", 10, 20);
        }

        let (p50, p95, p99) = collector.get_latency_percentiles();
        assert!(p50 > 0.0 && p50 <= 100.0);
        assert!(p95 > p50);
        assert!(p99 > p95);
    }

    #[test]
    fn test_model_stats() {
        let collector = MetricsCollector::new();

        collector.record_request_end(100.0, "model-a", 50, 100);
        collector.record_request_end(200.0, "model-b", 75, 150);
        collector.record_request_end(150.0, "model-a", 60, 120);

        let stats = collector.get_model_stats();
        assert_eq!(stats.len(), 2);

        let model_a = stats.iter().find(|(name, _)| name == "model-a");
        assert!(model_a.is_some());

        if let Some((_, stats)) = model_a {
            assert_eq!(stats.request_count.load(Ordering::Relaxed), 2);
            assert_eq!(stats.total_input_tokens.load(Ordering::Relaxed), 110);
            assert_eq!(stats.total_output_tokens.load(Ordering::Relaxed), 220);
        }
    }

    #[test]
    fn test_error_recording() {
        let collector = MetricsCollector::new();

        collector.record_error("timeout");
        collector.record_error("auth_failed");
        collector.record_error("timeout");

        assert_eq!(collector.total_errors.load(Ordering::Relaxed), 3);
        assert_eq!(collector.errors_by_type.len(), 2);
    }
}
