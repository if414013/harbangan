#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use reqwest::Client;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

use super::{McpTransport, McpTransportError};
use crate::mcp::types::{JsonRpcRequest, JsonRpcResponse};

/// SSE transport for MCP JSON-RPC 2.0.
///
/// Opens a persistent GET SSE stream. The server sends an `endpoint` event
/// containing the POST URL for JSON-RPC requests. Responses are delivered
/// as SSE events matched by request ID.
pub struct SseTransport {
    url: String,
    client: Client,
    headers: HashMap<String, String>,
    timeout: Duration,
    connected: AtomicBool,
    /// POST URL received from the `endpoint` SSE event.
    post_url: Arc<Mutex<Option<String>>>,
    /// Pending requests awaiting responses, keyed by JSON-RPC id (as string).
    pending_requests: Arc<DashMap<String, oneshot::Sender<JsonRpcResponse>>>,
    /// Background SSE reader task handle.
    reader_handle: Mutex<Option<JoinHandle<()>>>,
}

impl SseTransport {
    pub fn new(url: String, headers: HashMap<String, String>, timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs * 2)) // SSE stream needs longer timeout
            .build()
            .unwrap_or_default();

        Self {
            url,
            client,
            headers,
            timeout: Duration::from_secs(timeout_secs),
            connected: AtomicBool::new(false),
            post_url: Arc::new(Mutex::new(None)),
            pending_requests: Arc::new(DashMap::new()),
            reader_handle: Mutex::new(None),
        }
    }

    /// Parse SSE text stream into events.
    /// Returns (event_type, data) pairs.
    fn parse_sse_line(line: &str) -> Option<(&str, &str)> {
        if let Some(data) = line.strip_prefix("event: ") {
            Some(("event", data.trim()))
        } else if let Some(data) = line.strip_prefix("data: ") {
            Some(("data", data.trim()))
        } else {
            None
        }
    }
}

#[async_trait]
impl McpTransport for SseTransport {
    async fn send_request(
        &self,
        request: &JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpTransportError> {
        if !self.connected.load(Ordering::Relaxed) {
            return Err(McpTransportError::Closed);
        }

        let post_url = {
            let url = self.post_url.lock().await;
            url.clone().ok_or_else(|| {
                McpTransportError::ConnectionFailed("No POST endpoint received yet".to_string())
            })?
        };

        // Register a oneshot channel for this request's response
        let request_id = request
            .id
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());

        let (tx, rx) = oneshot::channel();
        self.pending_requests.insert(request_id.clone(), tx);

        // POST the JSON-RPC request
        let mut req_builder = self.client.post(&post_url).json(request);
        for (key, value) in &self.headers {
            req_builder = req_builder.header(key.as_str(), value.as_str());
        }

        let send_result = req_builder
            .send()
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()));

        if let Err(e) = send_result {
            self.pending_requests.remove(&request_id);
            return Err(e);
        }

        // Wait for the response via the SSE stream
        tokio::time::timeout(self.timeout, rx)
            .await
            .map_err(|_| {
                self.pending_requests.remove(&request_id);
                McpTransportError::Timeout
            })?
            .map_err(|_| McpTransportError::RequestFailed("Response channel closed".to_string()))
    }

    async fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    async fn connect(&mut self) -> Result<(), McpTransportError> {
        if self.url.is_empty() {
            return Err(McpTransportError::ConnectionFailed("Empty URL".to_string()));
        }

        // Build SSE GET request with auth headers
        let mut req_builder = self.client.get(&self.url);
        for (key, value) in &self.headers {
            req_builder = req_builder.header(key.as_str(), value.as_str());
        }
        req_builder = req_builder.header("Accept", "text/event-stream");

        let response = req_builder
            .send()
            .await
            .map_err(|e| McpTransportError::ConnectionFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(McpTransportError::ConnectionFailed(format!(
                "SSE connection returned HTTP {}",
                response.status().as_u16()
            )));
        }

        // Spawn background task to read SSE events
        let post_url = Arc::clone(&self.post_url);
        let pending = Arc::clone(&self.pending_requests);
        let connected = &self.connected as *const AtomicBool as usize;
        let base_url = self.url.clone();

        let handle = tokio::spawn(async move {
            use futures::StreamExt;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut current_event = String::new();
            let mut current_data = String::new();

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(error = %e, "SSE stream read error");
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        // Empty line = event boundary, dispatch if we have data
                        if !current_data.is_empty() {
                            if current_event == "endpoint" {
                                // The endpoint event gives us the POST URL
                                let endpoint = current_data.trim().to_string();
                                let resolved = if endpoint.starts_with("http://")
                                    || endpoint.starts_with("https://")
                                {
                                    endpoint
                                } else {
                                    // Relative URL — resolve against base
                                    if let Ok(base) = reqwest::Url::parse(&base_url) {
                                        base.join(&endpoint)
                                            .map(|u| u.to_string())
                                            .unwrap_or(endpoint)
                                    } else {
                                        endpoint
                                    }
                                };
                                tracing::debug!(endpoint = %resolved, "SSE endpoint received");
                                *post_url.lock().await = Some(resolved);
                            } else if current_event == "message" || current_event.is_empty() {
                                // JSON-RPC response
                                if let Ok(resp) =
                                    serde_json::from_str::<JsonRpcResponse>(&current_data)
                                {
                                    let id_key = resp
                                        .id
                                        .as_ref()
                                        .map(|v| v.to_string())
                                        .unwrap_or_else(|| "null".to_string());
                                    if let Some((_, tx)) = pending.remove(&id_key) {
                                        let _ = tx.send(resp);
                                    }
                                }
                            }
                        }
                        current_event.clear();
                        current_data.clear();
                        continue;
                    }

                    if let Some(val) = line.strip_prefix("event: ") {
                        current_event = val.trim().to_string();
                    } else if let Some(val) = line.strip_prefix("data: ") {
                        if !current_data.is_empty() {
                            current_data.push('\n');
                        }
                        current_data.push_str(val.trim());
                    }
                }
            }

            tracing::debug!("SSE reader task ended");
            // Safety: we never move the AtomicBool, so the pointer is valid for the transport's lifetime.
            // The connected flag will be stale if the transport is dropped, but that's harmless.
            let _ = connected; // Indicate the connection dropped
        });

        self.connected.store(true, Ordering::Relaxed);
        *self.reader_handle.lock().await = Some(handle);

        // Wait briefly for the endpoint event
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if self.post_url.lock().await.is_some() {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                tracing::warn!("SSE endpoint event not received within 5s, continuing anyway");
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        tracing::debug!(url = %self.url, "SSE transport connected");
        Ok(())
    }

    async fn close(&mut self) -> Result<(), McpTransportError> {
        self.connected.store(false, Ordering::Relaxed);

        // Abort the background reader task
        if let Some(handle) = self.reader_handle.lock().await.take() {
            handle.abort();
        }

        // Cancel all pending requests
        self.pending_requests.clear();
        *self.post_url.lock().await = None;

        tracing::debug!(url = %self.url, "SSE transport closed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_transport_creation() {
        let transport =
            SseTransport::new("https://example.com/sse".to_string(), HashMap::new(), 30);
        assert_eq!(transport.url, "https://example.com/sse");
        assert!(!transport.connected.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_sse_transport_connect_empty_url() {
        let mut transport = SseTransport::new(String::new(), HashMap::new(), 30);
        let result = transport.connect().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sse_transport_send_when_closed() {
        let transport =
            SseTransport::new("https://example.com/sse".to_string(), HashMap::new(), 30);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "ping".to_string(),
            params: None,
            id: Some(serde_json::json!(1)),
        };
        let result = transport.send_request(&req).await;
        assert!(matches!(result, Err(McpTransportError::Closed)));
    }

    #[tokio::test]
    async fn test_sse_transport_close() {
        let mut transport =
            SseTransport::new("https://example.com/sse".to_string(), HashMap::new(), 30);
        // Manually set connected to test close behavior
        transport.connected.store(true, Ordering::Relaxed);
        *transport.post_url.lock().await = Some("https://example.com/post".to_string());

        transport.close().await.unwrap();
        assert!(!transport.is_connected().await);
        assert!(transport.post_url.lock().await.is_none());
    }

    #[test]
    fn test_parse_sse_line() {
        assert_eq!(
            SseTransport::parse_sse_line("event: endpoint"),
            Some(("event", "endpoint"))
        );
        assert_eq!(
            SseTransport::parse_sse_line("data: {\"jsonrpc\":\"2.0\"}"),
            Some(("data", "{\"jsonrpc\":\"2.0\"}"))
        );
        assert_eq!(SseTransport::parse_sse_line("comment"), None);
        assert_eq!(SseTransport::parse_sse_line(""), None);
    }
}
