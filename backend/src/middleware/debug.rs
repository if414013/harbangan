#![allow(dead_code)]

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Local;
use http_body_util::BodyExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

use crate::config::DebugMode;
use crate::routes::AppState;

use once_cell::sync::Lazy;

/// Debug logger state for a single request
#[derive(Default)]
struct DebugRequestState {
    request_body: Option<Bytes>,
    kiro_request_body: Option<Bytes>,
    raw_chunks: Vec<Bytes>,
    modified_chunks: Vec<Bytes>,
    app_logs: Vec<String>,
}

/// Global debug logger instance
pub static DEBUG_LOGGER: Lazy<DebugLogger> = Lazy::new(DebugLogger::new);

/// Debug logger for capturing request/response data
///
/// Supports three modes:
/// - off: logging disabled
/// - errors: logs are saved only on errors (4xx, 5xx)
/// - all: logs are overwritten on each request
pub struct DebugLogger {
    debug_dir: PathBuf,
    state: Arc<RwLock<DebugRequestState>>,
    debug_mode: Arc<RwLock<DebugMode>>,
}

impl DebugLogger {
    /// Create a new debug logger
    fn new() -> Self {
        Self {
            debug_dir: PathBuf::from("debug_logs"),
            state: Arc::new(RwLock::new(DebugRequestState::default())),
            debug_mode: Arc::new(RwLock::new(DebugMode::Off)),
        }
    }

    /// Set the debug mode (called during app initialization)
    pub async fn set_mode(&self, mode: DebugMode) {
        let mut dm = self.debug_mode.write().await;
        *dm = mode;
    }

    /// Check if logging is enabled
    async fn is_enabled(&self) -> bool {
        let mode = self.debug_mode.read().await;
        !matches!(*mode, DebugMode::Off)
    }

    /// Check if immediate write mode (all mode)
    async fn is_immediate_write(&self) -> bool {
        let mode = self.debug_mode.read().await;
        matches!(*mode, DebugMode::All)
    }

    /// Prepare for a new request
    ///
    /// In "all" mode: clears the logs folder.
    /// In "errors" mode: clears buffers.
    pub async fn prepare_new_request(&self) {
        if !self.is_enabled().await {
            return;
        }

        // Clear state
        let mut state = self.state.write().await;
        *state = DebugRequestState::default();

        // In "all" mode, clear the debug directory
        if self.is_immediate_write().await {
            if let Err(e) = self.clear_debug_dir().await {
                tracing::warn!("[DebugLogger] Error preparing directory: {}", e);
            } else {
                tracing::debug!(
                    "[DebugLogger] Directory {} cleared for new request.",
                    self.debug_dir.display()
                );
            }
        }
    }

    /// Clear the debug directory
    async fn clear_debug_dir(&self) -> std::io::Result<()> {
        if self.debug_dir.exists() {
            fs::remove_dir_all(&self.debug_dir).await?;
        }
        fs::create_dir_all(&self.debug_dir).await?;
        Ok(())
    }

    /// Log request body (from client, OpenAI/Anthropic format)
    ///
    /// In "all" mode: writes immediately to file.
    /// In "errors" mode: buffers.
    pub async fn log_request_body(&self, body: Bytes) {
        if !self.is_enabled().await {
            return;
        }

        if self.is_immediate_write().await {
            if let Err(e) = self.write_request_body(&body).await {
                tracing::warn!("[DebugLogger] Error writing request_body: {}", e);
            }
        } else {
            let mut state = self.state.write().await;
            state.request_body = Some(body);
        }
    }

    /// Log Kiro request body (transformed request to Kiro API)
    ///
    /// In "all" mode: writes immediately to file.
    /// In "errors" mode: buffers.
    pub async fn log_kiro_request_body(&self, body: Bytes) {
        if !self.is_enabled().await {
            return;
        }

        if self.is_immediate_write().await {
            if let Err(e) = self.write_kiro_request_body(&body).await {
                tracing::warn!("[DebugLogger] Error writing kiro_request_body: {}", e);
            }
        } else {
            let mut state = self.state.write().await;
            state.kiro_request_body = Some(body);
        }
    }

    /// Log raw response chunk (from Kiro API)
    ///
    /// In "all" mode: appends immediately to file.
    /// In "errors" mode: buffers.
    pub async fn log_raw_chunk(&self, chunk: Bytes) {
        if !self.is_enabled().await {
            return;
        }

        if self.is_immediate_write().await {
            if let Err(_e) = self.append_raw_chunk(&chunk).await {
                // Don't log errors for chunk appending to avoid spam
            }
        } else {
            let mut state = self.state.write().await;
            state.raw_chunks.push(chunk);
        }
    }

    /// Log modified response chunk (to client)
    ///
    /// In "all" mode: appends immediately to file.
    /// In "errors" mode: buffers.
    pub async fn log_modified_chunk(&self, chunk: Bytes) {
        if !self.is_enabled().await {
            return;
        }

        if self.is_immediate_write().await {
            if let Err(_e) = self.append_modified_chunk(&chunk).await {
                // Don't log errors for chunk appending to avoid spam
            }
        } else {
            let mut state = self.state.write().await;
            state.modified_chunks.push(chunk);
        }
    }

    /// Log an application log message
    ///
    /// These are collected and written to app_logs.txt
    pub async fn log_app_message(&self, level: &str, module: &str, message: &str) {
        if !self.is_enabled().await {
            return;
        }

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_line = format!("{} | {:<8} | {} | {}", timestamp, level, module, message);

        if self.is_immediate_write().await {
            if (self.append_app_log(&log_line).await).is_err() {
                // Silently ignore errors to avoid recursion
            }
        } else {
            let mut state = self.state.write().await;
            state.app_logs.push(log_line);
        }
    }

    /// Log error information
    pub async fn log_error_info(&self, status_code: u16, error_message: &str) {
        if !self.is_enabled().await {
            return;
        }

        if let Err(e) = self.write_error_info(status_code, error_message).await {
            tracing::warn!("[DebugLogger] Error writing error_info: {}", e);
        } else {
            tracing::debug!("[DebugLogger] Error info saved (status={})", status_code);
        }
    }

    /// Flush buffers on error
    ///
    /// In "errors" mode: flushes buffers and saves error_info.
    /// In "all" mode: only saves error_info (data already written).
    pub async fn flush_on_error(&self, status_code: u16, error_message: &str) {
        if !self.is_enabled().await {
            return;
        }

        // In "all" mode, data is already written, just add error info and app logs
        if self.is_immediate_write().await {
            self.log_error_info(status_code, error_message).await;
            return;
        }

        let state = self.state.read().await;

        // Check if there's anything to flush
        if state.request_body.is_none()
            && state.kiro_request_body.is_none()
            && state.raw_chunks.is_empty()
            && state.modified_chunks.is_empty()
            && state.app_logs.is_empty()
        {
            return;
        }

        // Clear and recreate directory
        if let Err(e) = self.clear_debug_dir().await {
            tracing::warn!("[DebugLogger] Error clearing directory: {}", e);
            return;
        }

        // Write all buffered data
        if let Some(ref body) = state.request_body {
            if let Err(e) = self.write_request_body(body).await {
                tracing::warn!("[DebugLogger] Error writing request_body: {}", e);
            }
        }

        if let Some(ref body) = state.kiro_request_body {
            if let Err(e) = self.write_kiro_request_body(body).await {
                tracing::warn!("[DebugLogger] Error writing kiro_request_body: {}", e);
            }
        }

        if !state.raw_chunks.is_empty() {
            if let Err(e) = self.write_raw_chunks(&state.raw_chunks).await {
                tracing::warn!("[DebugLogger] Error writing raw chunks: {}", e);
            }
        }

        if !state.modified_chunks.is_empty() {
            if let Err(e) = self.write_modified_chunks(&state.modified_chunks).await {
                tracing::warn!("[DebugLogger] Error writing modified chunks: {}", e);
            }
        }

        if !state.app_logs.is_empty() {
            if let Err(e) = self.write_app_logs(&state.app_logs).await {
                tracing::warn!("[DebugLogger] Error writing app logs: {}", e);
            }
        }

        // Write error info
        drop(state); // Release read lock before calling log_error_info
        self.log_error_info(status_code, error_message).await;

        tracing::info!(
            "[DebugLogger] Error logs flushed to {} (status={})",
            self.debug_dir.display(),
            status_code
        );
    }

    /// Discard buffers without writing
    ///
    /// Called when request completed successfully in "errors" mode.
    /// In "all" mode, writes app logs even for successful requests.
    pub async fn discard_buffers(&self) {
        let mode = {
            let m = self.debug_mode.read().await;
            m.clone()
        };

        match mode {
            DebugMode::Errors => {
                let mut state = self.state.write().await;
                *state = DebugRequestState::default();
            }
            DebugMode::All => {
                // In "all" mode, app logs are already written via append
                // Just clear the state
                let mut state = self.state.write().await;
                *state = DebugRequestState::default();
            }
            DebugMode::Off => {}
        }
    }

    // ==================== Private file writing methods ====================

    /// Write request body to file (pretty-printed JSON if valid)
    async fn write_request_body(&self, body: &Bytes) -> std::io::Result<()> {
        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("request_body.json");

        // Try to parse as JSON for pretty printing
        if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(body) {
            let pretty =
                serde_json::to_string_pretty(&json_value).map_err(std::io::Error::other)?;
            fs::write(&file_path, pretty).await?;
        } else {
            // Write raw bytes if not valid JSON
            fs::write(&file_path, body).await?;
        }

        Ok(())
    }

    /// Write Kiro request body to file (pretty-printed JSON if valid)
    async fn write_kiro_request_body(&self, body: &Bytes) -> std::io::Result<()> {
        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("kiro_request_body.json");

        // Try to parse as JSON for pretty printing
        if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(body) {
            let pretty =
                serde_json::to_string_pretty(&json_value).map_err(std::io::Error::other)?;
            fs::write(&file_path, pretty).await?;
        } else {
            // Write raw bytes if not valid JSON
            fs::write(&file_path, body).await?;
        }

        Ok(())
    }

    /// Append raw chunk to file
    async fn append_raw_chunk(&self, chunk: &Bytes) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;

        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("response_stream_raw.txt");

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        file.write_all(chunk).await?;
        file.flush().await?;

        Ok(())
    }

    /// Append modified chunk to file
    async fn append_modified_chunk(&self, chunk: &Bytes) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;

        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("response_stream_modified.txt");

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        file.write_all(chunk).await?;
        file.flush().await?;

        Ok(())
    }

    /// Append app log line to file
    async fn append_app_log(&self, log_line: &str) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;

        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("app_logs.txt");

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        file.write_all(log_line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;

        Ok(())
    }

    /// Write raw chunks from buffer
    async fn write_raw_chunks(&self, chunks: &[Bytes]) -> std::io::Result<()> {
        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("response_stream_raw.txt");

        let mut data = Vec::new();
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }

        fs::write(&file_path, data).await?;
        Ok(())
    }

    /// Write modified chunks from buffer
    async fn write_modified_chunks(&self, chunks: &[Bytes]) -> std::io::Result<()> {
        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("response_stream_modified.txt");

        let mut data = Vec::new();
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }

        fs::write(&file_path, data).await?;
        Ok(())
    }

    /// Write app logs from buffer
    async fn write_app_logs(&self, logs: &[String]) -> std::io::Result<()> {
        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("app_logs.txt");

        let content = logs.join("\n");
        fs::write(&file_path, content).await?;
        Ok(())
    }

    /// Write error information to file
    async fn write_error_info(&self, status_code: u16, error_message: &str) -> std::io::Result<()> {
        fs::create_dir_all(&self.debug_dir).await?;
        let file_path = self.debug_dir.join("error_info.json");

        let error_info = serde_json::json!({
            "status_code": status_code,
            "error_message": error_message
        });

        let pretty = serde_json::to_string_pretty(&error_info).map_err(std::io::Error::other)?;
        fs::write(&file_path, pretty).await?;

        Ok(())
    }
}

/// Debug logging middleware
///
/// This middleware initializes debug logging BEFORE request processing,
/// which allows capturing validation errors in debug logs.
///
/// The middleware:
/// 1. Intercepts requests to API endpoints (/v1/chat/completions, /v1/messages)
/// 2. Calls prepare_new_request() to initialize buffers
/// 3. Reads and logs the raw request body
/// 4. Passes the request to the next handler
///
/// Flush/discard operations are handled by route handlers.
pub async fn debug_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    // Read config snapshot for debug mode check
    let debug_mode = state
        .config
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .debug_mode
        .clone();

    // Skip if debug mode is off
    if matches!(debug_mode, DebugMode::Off) {
        return next.run(request).await;
    }

    // Only log API endpoints
    let path = request.uri().path();
    let is_api_endpoint = path == "/v1/chat/completions" || path == "/v1/messages";

    if !is_api_endpoint {
        return next.run(request).await;
    }

    // Set debug mode on the global logger
    DEBUG_LOGGER.set_mode(debug_mode).await;

    // Prepare for new request (clears buffers/directory)
    DEBUG_LOGGER.prepare_new_request().await;

    // Extract and log request body
    let (parts, body) = request.into_parts();
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            tracing::warn!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    // Log request body
    DEBUG_LOGGER.log_request_body(body_bytes.clone()).await;

    // Reconstruct request with body
    let request = Request::from_parts(parts, Body::from(body_bytes));

    // Process request
    let response = next.run(request).await;

    // Note: flush_on_error() and discard_buffers() are called by route handlers
    // This matches Python's behavior where routes control when to flush

    response
}
