//! Report generation for benchmark results.

use super::metrics::MetricsSnapshot;
use serde::{Deserialize, Serialize};

/// Complete benchmark report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Results per concurrency level
    pub results: Vec<ConcurrencyResult>,
    /// Optimal concurrency level (best RPS with acceptable latency)
    pub optimal_concurrency: Option<usize>,
    /// Maximum RPS achieved
    pub max_rps: f64,
    /// Concurrency level at max RPS
    pub max_rps_concurrency: usize,
}

/// Results for a single concurrency level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyResult {
    pub concurrency: usize,
    pub requests_per_second: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub ttfb_p50_ms: f64,
    pub ttfb_p95_ms: f64,
    pub success_rate: f64,
    pub total_requests: u64,
    pub errors: u64,
    pub avg_cpu: f32,
    pub max_cpu: f32,
    pub avg_memory_mb: f64,
    pub max_memory_mb: f64,
}

impl BenchmarkReport {
    /// Create a report from benchmark results
    pub fn from_results(results: Vec<(usize, MetricsSnapshot)>) -> Self {
        let concurrency_results: Vec<ConcurrencyResult> = results
            .iter()
            .map(|(concurrency, snapshot)| ConcurrencyResult {
                concurrency: *concurrency,
                requests_per_second: snapshot.requests_per_second,
                latency_p50_ms: snapshot.latency_p50,
                latency_p95_ms: snapshot.latency_p95,
                latency_p99_ms: snapshot.latency_p99,
                ttfb_p50_ms: snapshot.ttfb_p50,
                ttfb_p95_ms: snapshot.ttfb_p95,
                success_rate: snapshot.success_rate,
                total_requests: snapshot.success_count + snapshot.error_count,
                errors: snapshot.error_count,
                avg_cpu: snapshot.avg_cpu,
                max_cpu: snapshot.max_cpu,
                avg_memory_mb: snapshot.avg_memory_mb,
                max_memory_mb: snapshot.max_memory_mb,
            })
            .collect();

        // Find max RPS
        let (max_rps, max_rps_concurrency) = concurrency_results
            .iter()
            .map(|r| (r.requests_per_second, r.concurrency))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0.0, 0));

        // Find optimal concurrency (highest RPS with p99 < 500ms and success > 99%)
        let optimal_concurrency = concurrency_results
            .iter()
            .filter(|r| r.latency_p99_ms < 500.0 && r.success_rate > 99.0)
            .max_by(|a, b| {
                a.requests_per_second
                    .partial_cmp(&b.requests_per_second)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.concurrency);

        Self {
            results: concurrency_results,
            optimal_concurrency,
            max_rps,
            max_rps_concurrency,
        }
    }

    /// Print the report as an ASCII table
    pub fn print_table(&self) {
        println!();
        println!("╔════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
        println!("║                              KIRO GATEWAY BENCHMARK RESULTS                                            ║");
        println!("╚════════════════════════════════════════════════════════════════════════════════════════════════════════╝");
        println!();
        println!("┌────────────┬──────────┬──────────┬──────────┬──────────┬──────────┬──────────┬──────────┬──────────────┐");
        println!("│ Concurrency│   RPS    │  p50(ms) │  p95(ms) │  p99(ms) │ TTFB p50 │ Success% │ CPU(avg) │  Memory(MB)  │");
        println!("├────────────┼──────────┼──────────┼──────────┼──────────┼──────────┼──────────┼──────────┼──────────────┤");

        for result in &self.results {
            println!(
                "│ {:>10} │ {:>8.1} │ {:>8.1} │ {:>8.1} │ {:>8.1} │ {:>8.1} │ {:>7.1}% │ {:>6.1}%  │ {:>6.0}/{:<6.0} │",
                result.concurrency,
                result.requests_per_second,
                result.latency_p50_ms,
                result.latency_p95_ms,
                result.latency_p99_ms,
                result.ttfb_p50_ms,
                result.success_rate,
                result.avg_cpu,
                result.avg_memory_mb,
                result.max_memory_mb
            );
        }

        println!("└────────────┴──────────┴──────────┴──────────┴──────────┴──────────┴──────────┴──────────┴──────────────┘");
        println!();

        if let Some(optimal) = self.optimal_concurrency {
            let optimal_result = self.results.iter().find(|r| r.concurrency == optimal);
            if let Some(r) = optimal_result {
                println!(
                    "Optimal concurrency: {} (RPS: {:.1}, p99: {:.1}ms)",
                    optimal, r.requests_per_second, r.latency_p99_ms
                );
            }
        }

        println!(
            "Maximum RPS: {:.1} at concurrency {}",
            self.max_rps, self.max_rps_concurrency
        );
        println!();
    }

    /// Export the report as JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Print a compact summary
    pub fn print_summary(&self) {
        println!("\n=== Benchmark Summary ===");
        println!(
            "Max RPS: {:.1} at concurrency {}",
            self.max_rps, self.max_rps_concurrency
        );

        if let Some(optimal) = self.optimal_concurrency {
            println!("Optimal concurrency: {}", optimal);
        } else {
            println!("Optimal concurrency: N/A (no level met criteria)");
        }

        if let Some(first) = self.results.first() {
            if let Some(last) = self.results.last() {
                println!(
                    "RPS scaling: {:.1}x from {} to {} concurrency",
                    last.requests_per_second / first.requests_per_second.max(1.0),
                    first.concurrency,
                    last.concurrency
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_generation() {
        let results = vec![
            (
                10,
                MetricsSnapshot {
                    success_count: 100,
                    error_count: 0,
                    success_rate: 100.0,
                    requests_per_second: 150.0,
                    latency_p50: 65.0,
                    latency_p95: 89.0,
                    latency_p99: 102.0,
                    ttfb_p50: 52.0,
                    ttfb_p95: 70.0,
                    ttfb_p99: 85.0,
                    bytes_received: 10000,
                    elapsed_secs: 0.67,
                    avg_cpu: 25.0,
                    max_cpu: 45.0,
                    avg_memory_mb: 100.0,
                    max_memory_mb: 120.0,
                },
            ),
            (
                50,
                MetricsSnapshot {
                    success_count: 500,
                    error_count: 1,
                    success_rate: 99.8,
                    requests_per_second: 680.0,
                    latency_p50: 72.0,
                    latency_p95: 110.0,
                    latency_p99: 145.0,
                    ttfb_p50: 55.0,
                    ttfb_p95: 80.0,
                    ttfb_p99: 100.0,
                    bytes_received: 50000,
                    elapsed_secs: 0.74,
                    avg_cpu: 55.0,
                    max_cpu: 80.0,
                    avg_memory_mb: 150.0,
                    max_memory_mb: 200.0,
                },
            ),
        ];

        let report = BenchmarkReport::from_results(results);

        assert_eq!(report.results.len(), 2);
        assert_eq!(report.max_rps_concurrency, 50);
        assert!((report.max_rps - 680.0).abs() < 0.1);
    }
}
