//! Mock Kiro API server that generates AWS Event Stream format responses.

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use bytes::{BufMut, BytesMut};
use rand::Rng;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::config::MockServerConfig;

/// Mock Kiro API server for benchmarking
pub struct MockKiroServer {
    config: MockServerConfig,
    shutdown_tx: Option<oneshot::Sender<()>>,
    port: u16,
}

impl MockKiroServer {
    /// Create a new mock server with the given configuration
    pub fn new(config: MockServerConfig) -> Self {
        Self {
            config,
            shutdown_tx: None,
            port: 0,
        }
    }

    /// Start the mock server and return the actual port
    pub async fn start(&mut self) -> anyhow::Result<u16> {
        let addr = format!("127.0.0.1:{}", self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        let port = listener.local_addr()?.port();
        self.port = port;

        let config = Arc::new(self.config.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let app = Router::new()
            .route("/generateAssistantResponse", post(handle_generate))
            .with_state(config);

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        // Give the server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        Ok(port)
    }

    /// Get the server's port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the server's URL
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Stop the mock server
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for MockKiroServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Handle generateAssistantResponse requests
async fn handle_generate(State(config): State<Arc<MockServerConfig>>) -> Response {
    // Simulate random errors
    if config.error_rate > 0.0 {
        let mut rng = rand::thread_rng();
        if rng.gen::<f64>() < config.error_rate {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Simulated error").into_response();
        }
    }

    if config.streaming {
        // Generate streaming response
        let stream = generate_stream(config);
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/vnd.amazon.eventstream")
            .body(Body::from_stream(stream))
            .unwrap()
    } else {
        // Generate non-streaming response
        let content = generate_content(config.chunk_count * config.chunk_size);
        let response = serde_json::json!({
            "conversationId": uuid::Uuid::new_v4().to_string(),
            "assistantResponseMessage": {
                "content": [{"type": "text", "text": content}]
            },
            "usage": {
                "inputTokens": 100,
                "outputTokens": content.len() / 4
            }
        });

        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&response).unwrap(),
        )
            .into_response()
    }
}

/// Generate a streaming response using AWS Event Stream format
fn generate_stream(
    config: Arc<MockServerConfig>,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> {
    async_stream::stream! {
        let chunk_latency = tokio::time::Duration::from_millis(config.chunk_latency_ms);

        // Send content chunks
        for i in 0..config.chunk_count {
            tokio::time::sleep(chunk_latency).await;

            let content = generate_content(config.chunk_size);
            let event = serde_json::json!({"content": content});
            let event_bytes = wrap_in_event_stream(&event);

            yield Ok(event_bytes);

            // Occasionally yield to allow other tasks to run
            if i % 5 == 0 {
                tokio::task::yield_now().await;
            }
        }

        // Send usage event
        let usage_event = serde_json::json!({
            "usage": {
                "inputTokens": 100,
                "outputTokens": config.chunk_count * config.chunk_size / 4
            }
        });
        yield Ok(wrap_in_event_stream(&usage_event));
    }
}

/// Wrap a JSON event in AWS Event Stream binary format
fn wrap_in_event_stream(event: &serde_json::Value) -> bytes::Bytes {
    let json_bytes = serde_json::to_vec(event).unwrap();

    // AWS Event Stream format:
    // - 4 bytes: total length (big-endian)
    // - 4 bytes: headers length (big-endian)
    // - 4 bytes: prelude CRC (we'll use 0 for simplicity)
    // - headers (we'll skip for simplicity)
    // - payload (JSON)
    // - 4 bytes: message CRC (we'll use 0 for simplicity)

    let headers_len = 0u32;
    let payload_len = json_bytes.len() as u32;
    let total_len = 16 + headers_len + payload_len; // 16 = prelude(12) + message_crc(4)

    let mut buf = BytesMut::with_capacity(total_len as usize);

    // Prelude
    buf.put_u32(total_len);
    buf.put_u32(headers_len);
    buf.put_u32(0); // prelude CRC

    // Payload
    buf.put_slice(&json_bytes);

    // Message CRC
    buf.put_u32(0);

    buf.freeze()
}

/// Generate random content of the specified size
fn generate_content(size: usize) -> String {
    const WORDS: &[&str] = &[
        "the",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "lazy",
        "dog",
        "hello",
        "world",
        "rust",
        "is",
        "awesome",
        "benchmark",
        "test",
        "performance",
        "gateway",
        "proxy",
        "api",
        "stream",
        "response",
    ];

    let mut rng = rand::thread_rng();
    let mut result = String::with_capacity(size);

    while result.len() < size {
        let word = WORDS[rng.gen_range(0..WORDS.len())];
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(word);
    }

    result.truncate(size);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_starts() {
        let config = MockServerConfig {
            port: 0,
            chunk_latency_ms: 1,
            chunk_count: 5,
            chunk_size: 10,
            error_rate: 0.0,
            streaming: true,
        };

        let mut server = MockKiroServer::new(config);
        let port = server.start().await.unwrap();
        assert!(port > 0);

        // Make a request
        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{}/generateAssistantResponse",
                port
            ))
            .json(&serde_json::json!({"test": true}))
            .send()
            .await
            .unwrap();

        assert!(resp.status().is_success());
        server.stop();
    }
}
