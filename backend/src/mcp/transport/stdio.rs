#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

use super::{McpTransport, McpTransportError};
use crate::mcp::types::{JsonRpcRequest, JsonRpcResponse, McpStdioConfig};

/// Allowed commands for STDIO transport. Only these binaries may be spawned.
const ALLOWED_COMMANDS: &[&str] = &["npx", "node", "python", "python3", "uvx", "docker"];

/// Environment variables that must never be passed to child processes.
const BLOCKED_ENV_VARS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
];

/// STDIO transport for MCP JSON-RPC 2.0.
///
/// Spawns a child process and communicates via newline-delimited JSON-RPC
/// over stdin (write) and stdout (read).
pub struct StdioTransport {
    stdio_config: McpStdioConfig,
    timeout: Duration,
    connected: AtomicBool,
    /// Stdin writer (shared for concurrent request sends).
    stdin: Arc<Mutex<Option<tokio::process::ChildStdin>>>,
    /// Pending requests awaiting responses, keyed by JSON-RPC id (as string).
    pending_requests: Arc<DashMap<String, oneshot::Sender<JsonRpcResponse>>>,
    /// Background stdout reader task handle.
    reader_handle: Mutex<Option<JoinHandle<()>>>,
    /// Child process handle for cleanup.
    child: Mutex<Option<Child>>,
}

impl StdioTransport {
    pub fn new(stdio_config: McpStdioConfig, timeout_secs: u64) -> Self {
        Self {
            stdio_config,
            timeout: Duration::from_secs(timeout_secs),
            connected: AtomicBool::new(false),
            stdin: Arc::new(Mutex::new(None)),
            pending_requests: Arc::new(DashMap::new()),
            reader_handle: Mutex::new(None),
            child: Mutex::new(None),
        }
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send_request(
        &self,
        request: &JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpTransportError> {
        if !self.connected.load(Ordering::Relaxed) {
            return Err(McpTransportError::Closed);
        }

        let request_id = request
            .id
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());

        // Register pending response channel
        let (tx, rx) = oneshot::channel();
        self.pending_requests.insert(request_id.clone(), tx);

        // Serialize and write to stdin
        let mut json_line = serde_json::to_string(request)
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
        json_line.push('\n');

        {
            let mut stdin_guard = self.stdin.lock().await;
            let stdin = stdin_guard.as_mut().ok_or(McpTransportError::Closed)?;
            stdin
                .write_all(json_line.as_bytes())
                .await
                .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
            stdin
                .flush()
                .await
                .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
        }

        // Wait for response with timeout
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
        if self.stdio_config.command.is_empty() {
            return Err(McpTransportError::ConnectionFailed(
                "Empty command".to_string(),
            ));
        }

        // Validate command against allowlist
        let base_command = std::path::Path::new(&self.stdio_config.command)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.stdio_config.command);
        if !ALLOWED_COMMANDS.contains(&base_command) {
            return Err(McpTransportError::ConnectionFailed(format!(
                "Command '{}' is not in the allowlist. Allowed: {}",
                self.stdio_config.command,
                ALLOWED_COMMANDS.join(", ")
            )));
        }

        // Block dangerous environment variables
        for key in self.stdio_config.envs.keys() {
            if BLOCKED_ENV_VARS.contains(&key.as_str()) {
                return Err(McpTransportError::ConnectionFailed(format!(
                    "Environment variable '{}' is blocked for security reasons",
                    key
                )));
            }
        }

        let mut cmd = Command::new(&self.stdio_config.command);
        cmd.args(&self.stdio_config.args);
        for (key, value) in &self.stdio_config.envs {
            cmd.env(key, value);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| McpTransportError::ProcessError(e.to_string()))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            McpTransportError::ProcessError("Failed to capture stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            McpTransportError::ProcessError("Failed to capture stdout".to_string())
        })?;

        *self.stdin.lock().await = Some(stdin);

        // Spawn background task to read stdout line-by-line
        let pending = Arc::clone(&self.pending_requests);
        let connected_ptr = &self.connected as *const AtomicBool as usize;

        let handle = tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<JsonRpcResponse>(&line) {
                            Ok(resp) => {
                                let id_key = resp
                                    .id
                                    .as_ref()
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "null".to_string());
                                if let Some((_, tx)) = pending.remove(&id_key) {
                                    let _ = tx.send(resp);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    line = %line,
                                    "Failed to parse JSON-RPC response from stdout"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::debug!("STDIO stdout stream ended");
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "STDIO stdout read error");
                        break;
                    }
                }
            }

            // Stdout closed means process likely exited
            let _ = connected_ptr;
        });

        *self.reader_handle.lock().await = Some(handle);
        *self.child.lock().await = Some(child);
        self.connected.store(true, Ordering::Relaxed);

        tracing::debug!(
            command = %self.stdio_config.command,
            "STDIO transport connected"
        );
        Ok(())
    }

    async fn close(&mut self) -> Result<(), McpTransportError> {
        self.connected.store(false, Ordering::Relaxed);

        // Drop stdin to send EOF
        *self.stdin.lock().await = None;

        // Abort the background reader task
        if let Some(handle) = self.reader_handle.lock().await.take() {
            handle.abort();
        }

        // Wait briefly for the child process, then kill if needed
        if let Some(mut child) = self.child.lock().await.take() {
            match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                Ok(Ok(status)) => {
                    tracing::debug!(
                        command = %self.stdio_config.command,
                        status = %status,
                        "STDIO child process exited"
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        error = %e,
                        "Error waiting for STDIO child process"
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        command = %self.stdio_config.command,
                        "STDIO child process did not exit in time, killing"
                    );
                    let _ = child.kill().await;
                }
            }
        }

        // Cancel all pending requests
        self.pending_requests.clear();

        tracing::debug!(
            command = %self.stdio_config.command,
            "STDIO transport closed"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> McpStdioConfig {
        McpStdioConfig {
            command: "node".to_string(),
            args: vec!["-e".to_string(), "console.log('hello')".to_string()],
            envs: HashMap::new(),
        }
    }

    #[test]
    fn test_stdio_transport_creation() {
        let transport = StdioTransport::new(test_config(), 30);
        assert!(!transport.connected.load(Ordering::Relaxed));
        assert_eq!(transport.stdio_config.command, "node");
    }

    #[tokio::test]
    async fn test_stdio_transport_rejects_disallowed_command() {
        let config = McpStdioConfig {
            command: "rm".to_string(),
            args: vec!["-rf".to_string(), "/".to_string()],
            envs: HashMap::new(),
        };
        let mut transport = StdioTransport::new(config, 30);
        let result = transport.connect().await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not in the allowlist"));
    }

    #[tokio::test]
    async fn test_stdio_transport_blocks_dangerous_env_vars() {
        let config = McpStdioConfig {
            command: "node".to_string(),
            args: vec![],
            envs: HashMap::from([("LD_PRELOAD".to_string(), "/tmp/evil.so".to_string())]),
        };
        let mut transport = StdioTransport::new(config, 30);
        let result = transport.connect().await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("blocked for security reasons"));
    }

    #[tokio::test]
    async fn test_stdio_transport_connect_empty_command() {
        let config = McpStdioConfig {
            command: String::new(),
            args: vec![],
            envs: HashMap::new(),
        };
        let mut transport = StdioTransport::new(config, 30);
        let result = transport.connect().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stdio_transport_send_when_closed() {
        let transport = StdioTransport::new(test_config(), 30);
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
    async fn test_stdio_transport_connect_and_close() {
        // Use 'node' as a process that keeps stdin open
        let config = McpStdioConfig {
            command: "node".to_string(),
            args: vec!["-e".to_string(), "process.stdin.resume()".to_string()],
            envs: HashMap::new(),
        };
        let mut transport = StdioTransport::new(config, 30);

        let result = transport.connect().await;
        assert!(result.is_ok());
        assert!(transport.is_connected().await);

        transport.close().await.unwrap();
        assert!(!transport.is_connected().await);
    }
}
