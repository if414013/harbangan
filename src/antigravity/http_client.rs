//! Antigravity HTTP client for Cloud Code API.
//!
//! Provides a streaming HTTP client with endpoint fallback and retry logic
//! for the Cloud Code `streamGenerateContent` endpoint.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::HeaderMap;
use reqwest::Client;
use thiserror::Error;
use tracing::{debug, error, warn};

use super::constants::{ENDPOINT_DAILY, ENDPOINT_PROD};

// === Error Types ===

#[derive(Error, Debug)]
pub enum CloudCodeError {
    #[error("Authentication failed (401): {0}")]
    Unauthorized(String),

    #[error("Bad request (400): {0}")]
    BadRequest(String),

    #[error("Rate limited (429)")]
    RateLimited,

    #[error("Server error ({status}): {message}")]
    ServerError { status: u16, message: String },

    #[error("All endpoints failed: {0}")]
    AllEndpointsFailed(String),

    #[error("Network error: {0}")]
    Network(String),
}

// === Retry Logic ===

/// Determines what action to take for a given HTTP status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAction {
    /// Request succeeded.
    Success,
    /// Refresh the auth token and retry once.
    RefreshToken,
    /// Retry with exponential backoff.
    Backoff,
    /// Fail immediately, do not retry.
    Fail,
}

/// Returns the retry action for a given HTTP status code.
pub fn classify_status(status: u16) -> RetryAction {
    match status {
        200..=299 => RetryAction::Success,
        400 => RetryAction::Fail,
        401 => RetryAction::RefreshToken,
        429 => RetryAction::Backoff,
        500..=599 => RetryAction::Backoff,
        _ => RetryAction::Fail,
    }
}

/// Calculates exponential backoff delay in milliseconds.
///
/// Formula: `base_ms * 2^attempt` with 10% jitter.
pub fn backoff_delay_ms(base_ms: u64, attempt: u32) -> u64 {
    let delay = base_ms.saturating_mul(2_u64.saturating_pow(attempt));
    let jitter = (delay as f64 * 0.1 * simple_random()) as u64;
    delay.saturating_add(jitter)
}

fn simple_random() -> f64 {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    let state = RandomState::new();
    (state.hash_one(std::time::SystemTime::now()) % 1000) as f64 / 1000.0
}

// === Streaming URL ===

/// Builds the streaming URL for a given endpoint.
pub fn streaming_url(endpoint: &str) -> String {
    format!("{}/v1internal:streamGenerateContent?alt=sse", endpoint)
}

/// Endpoint fallback order: daily first, then prod.
pub const ENDPOINTS: &[&str] = &[ENDPOINT_DAILY, ENDPOINT_PROD];

// === Client ===

/// HTTP client for Cloud Code API with endpoint fallback and retry logic.
pub struct CloudCodeClient {
    client: Client,
    max_retries: u32,
}

impl CloudCodeClient {
    /// Creates a new Cloud Code HTTP client with connection pooling.
    pub fn new(max_retries: u32) -> Result<Self> {
        let client = Client::builder()
            .pool_max_idle_per_host(10)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create Cloud Code HTTP client")?;

        Ok(Self {
            client,
            max_retries,
        })
    }

    /// Sends a streaming request to Cloud Code with endpoint fallback.
    ///
    /// Tries each endpoint in order. For each endpoint, applies retry
    /// logic based on the response status code:
    /// - 401 → returns `Unauthorized` (caller should refresh token)
    /// - 400 → returns `BadRequest` immediately
    /// - 429/5xx → retries with exponential backoff
    /// - Success → returns the `Response` for SSE parsing
    pub async fn send_streaming_request(
        &self,
        headers: HeaderMap,
        body: serde_json::Value,
    ) -> std::result::Result<reqwest::Response, CloudCodeError> {
        let body_str = body.to_string();
        let mut last_error = String::new();

        for endpoint in ENDPOINTS {
            let url = streaming_url(endpoint);
            debug!(endpoint, url = %url, "Trying Cloud Code endpoint");

            match self.try_endpoint(&url, &headers, &body_str).await {
                Ok(response) => return Ok(response),
                Err(CloudCodeError::BadRequest(msg)) => {
                    return Err(CloudCodeError::BadRequest(msg));
                }
                Err(CloudCodeError::Unauthorized(msg)) => {
                    return Err(CloudCodeError::Unauthorized(msg));
                }
                Err(e) => {
                    warn!(endpoint, error = %e, "Endpoint failed, trying next");
                    last_error = e.to_string();
                }
            }
        }

        Err(CloudCodeError::AllEndpointsFailed(last_error))
    }

    /// Tries a single endpoint with retry logic.
    ///
    /// Retries 429 and 5xx up to `max_retries` times.
    /// Returns immediately on 400 or 401.
    async fn try_endpoint(
        &self,
        url: &str,
        headers: &HeaderMap,
        body: &str,
    ) -> std::result::Result<reqwest::Response, CloudCodeError> {
        let mut attempt = 0u32;

        loop {
            let result = self
                .client
                .post(url)
                .headers(headers.clone())
                .body(body.to_string())
                .send()
                .await;

            match result {
                Ok(response) => {
                    let status = response.status().as_u16();

                    match classify_status(status) {
                        RetryAction::Success => return Ok(response),

                        RetryAction::Fail => {
                            let body_text = response.text().await.unwrap_or_default();
                            if status == 400 {
                                error!(status, body = %body_text, "Bad request");
                                return Err(CloudCodeError::BadRequest(body_text));
                            }
                            return Err(CloudCodeError::ServerError {
                                status,
                                message: body_text,
                            });
                        }

                        RetryAction::RefreshToken => {
                            let body_text = response.text().await.unwrap_or_default();
                            warn!("Received 401, caller should refresh token");
                            return Err(CloudCodeError::Unauthorized(body_text));
                        }

                        RetryAction::Backoff => {
                            let delay = if status == 429 {
                                RETRY_DELAY_429
                            } else {
                                RETRY_DELAY_5XX
                            };

                            if attempt < self.max_retries {
                                warn!(
                                    status,
                                    attempt = attempt + 1,
                                    max_retries = self.max_retries,
                                    "Retryable error, backing off"
                                );
                                tokio::time::sleep(delay).await;
                                attempt += 1;
                                continue;
                            }

                            let body_text = response.text().await.unwrap_or_default();
                            error!(status, attempts = attempt + 1, "Max retries exceeded");
                            if status == 429 {
                                return Err(CloudCodeError::RateLimited);
                            }
                            return Err(CloudCodeError::ServerError {
                                status,
                                message: body_text,
                            });
                        }
                    }
                }

                Err(e) => {
                    if attempt < self.max_retries && e.is_connect() {
                        warn!(
                            error = %e,
                            attempt = attempt + 1,
                            "Connection error, retrying"
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        attempt += 1;
                        continue;
                    }

                    error!(error = %e, url, "Request failed");
                    return Err(CloudCodeError::Network(e.to_string()));
                }
            }
        }
    }

    /// Returns a reference to the underlying HTTP client.
    pub fn client(&self) -> &Client {
        &self.client
    }
}

// === Retry Constants ===

const RETRY_DELAY_429: Duration = Duration::from_secs(2);
const RETRY_DELAY_5XX: Duration = Duration::from_secs(1);

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_status_success() {
        assert_eq!(classify_status(200), RetryAction::Success);
        assert_eq!(classify_status(201), RetryAction::Success);
        assert_eq!(classify_status(299), RetryAction::Success);
    }

    #[test]
    fn test_classify_status_fail() {
        assert_eq!(classify_status(400), RetryAction::Fail);
        assert_eq!(classify_status(403), RetryAction::Fail);
        assert_eq!(classify_status(404), RetryAction::Fail);
    }

    #[test]
    fn test_classify_status_refresh_token() {
        assert_eq!(classify_status(401), RetryAction::RefreshToken);
    }

    #[test]
    fn test_classify_status_backoff() {
        assert_eq!(classify_status(429), RetryAction::Backoff);
        assert_eq!(classify_status(500), RetryAction::Backoff);
        assert_eq!(classify_status(502), RetryAction::Backoff);
        assert_eq!(classify_status(503), RetryAction::Backoff);
        assert_eq!(classify_status(599), RetryAction::Backoff);
    }

    #[test]
    fn test_streaming_url() {
        let url = streaming_url("https://daily-cloudcode-pa.googleapis.com");
        assert_eq!(
            url,
            "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn test_streaming_url_no_trailing_slash() {
        let url = streaming_url("https://example.com");
        assert!(url.starts_with("https://example.com/"));
        assert!(url.contains("streamGenerateContent"));
    }

    #[test]
    fn test_endpoints_order() {
        assert_eq!(ENDPOINTS.len(), 2);
        assert_eq!(ENDPOINTS[0], ENDPOINT_DAILY);
        assert_eq!(ENDPOINTS[1], ENDPOINT_PROD);
    }

    #[test]
    fn test_backoff_delay_increases() {
        let d0 = backoff_delay_ms(1000, 0);
        let d1 = backoff_delay_ms(1000, 1);
        let d2 = backoff_delay_ms(1000, 2);

        // Each should be roughly double (with up to 10% jitter)
        assert!(d0 >= 1000 && d0 <= 1200);
        assert!(d1 >= 2000 && d1 <= 2400);
        assert!(d2 >= 4000 && d2 <= 4800);
    }

    #[test]
    fn test_backoff_delay_saturates() {
        // Should not panic on large attempt values
        let d = backoff_delay_ms(1000, 63);
        assert!(d > 0);
    }

    #[test]
    fn test_client_creation() {
        let client = CloudCodeClient::new(3);
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.max_retries, 3);
    }
}
