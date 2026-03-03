//! Benchmark module for Kiro Gateway performance testing.
//!
//! This module provides tools for measuring gateway performance including:
//! - Mock Kiro API server (AWS Event Stream format)
//! - Benchmark runner with concurrency control
//! - HdrHistogram-based metrics collection
//! - Report generation

pub mod config;
pub mod metrics;
pub mod mock_server;
pub mod report;
pub mod runner;

pub use config::{BenchmarkConfig, MockServerConfig};
pub use metrics::MetricsCollector;
pub use mock_server::MockKiroServer;
pub use report::BenchmarkReport;
pub use runner::BenchmarkRunner;
