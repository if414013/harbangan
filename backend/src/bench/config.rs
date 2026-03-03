//! Configuration structs for benchmarking.

use serde::{Deserialize, Serialize};

/// API format to use for benchmark requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ApiFormat {
    #[default]
    OpenAI,
    Anthropic,
}

impl std::fmt::Display for ApiFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiFormat::OpenAI => write!(f, "openai"),
            ApiFormat::Anthropic => write!(f, "anthropic"),
        }
    }
}

impl std::str::FromStr for ApiFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(ApiFormat::OpenAI),
            "anthropic" => Ok(ApiFormat::Anthropic),
            _ => Err(format!("Unknown API format: {}", s)),
        }
    }
}

/// Configuration for the mock Kiro server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockServerConfig {
    /// Port to listen on (0 for random)
    pub port: u16,
    /// Simulated latency per chunk in milliseconds
    pub chunk_latency_ms: u64,
    /// Number of content chunks to generate
    pub chunk_count: usize,
    /// Size of each content chunk in characters
    pub chunk_size: usize,
    /// Error rate (0.0 to 1.0)
    pub error_rate: f64,
    /// Whether to simulate streaming responses
    pub streaming: bool,
}

impl Default for MockServerConfig {
    fn default() -> Self {
        Self {
            port: 0,
            chunk_latency_ms: 10,
            chunk_count: 20,
            chunk_size: 50,
            error_rate: 0.0,
            streaming: true,
        }
    }
}

/// Configuration for a benchmark run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Gateway URL to benchmark
    pub gateway_url: String,
    /// API key for authentication
    pub api_key: String,
    /// Concurrency levels to test
    pub concurrency_levels: Vec<usize>,
    /// Duration per concurrency level in seconds
    pub duration_secs: u64,
    /// Whether to use streaming mode
    pub streaming: bool,
    /// API format (OpenAI or Anthropic)
    pub format: ApiFormat,
    /// Model to request
    pub model: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Warmup requests before measuring
    pub warmup_requests: usize,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            gateway_url: "http://localhost:8000".to_string(),
            api_key: "benchmark-key".to_string(),
            concurrency_levels: vec![10, 50, 100, 200],
            duration_secs: 30,
            streaming: true,
            format: ApiFormat::OpenAI,
            model: "claude-sonnet-4-20250514".to_string(),
            timeout_secs: 60,
            warmup_requests: 10,
        }
    }
}

impl BenchmarkConfig {
    /// Create config for standalone mode (mock server + gateway)
    #[allow(dead_code)]
    pub fn standalone(_mock_port: u16, gateway_port: u16) -> Self {
        Self {
            gateway_url: format!("http://127.0.0.1:{}", gateway_port),
            api_key: "benchmark-key".to_string(),
            ..Default::default()
        }
    }

    /// Get the appropriate endpoint path based on format
    pub fn endpoint_path(&self) -> &'static str {
        match self.format {
            ApiFormat::OpenAI => "/v1/chat/completions",
            ApiFormat::Anthropic => "/v1/messages",
        }
    }
}
