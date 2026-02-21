//! Local HTTP callback server for OAuth authorization code capture.
//!
//! Starts a temporary HTTP server on localhost to receive the Google OAuth
//! redirect with the authorization code, matching the antigravity-claude-proxy
//! callback server behavior.

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::constants::OAUTH_CALLBACK_PORT;

/// Fallback ports if the primary port is unavailable.
const FALLBACK_PORTS: &[u16] = &[51122, 51123, 51124, 51125, 51126];

/// Result of the OAuth callback server.
#[derive(Debug)]
pub struct CallbackResult {
    /// The authorization code from Google.
    pub code: String,
    /// The actual port the server bound to.
    pub port: u16,
}

/// Starts a local HTTP server to capture the OAuth callback.
///
/// Tries the primary port first, then falls back to alternatives.
/// Returns the authorization code once received, or an error on timeout.
///
/// # Arguments
/// * `expected_state` - The state parameter for CSRF protection.
/// * `timeout` - Maximum time to wait for the callback.
pub async fn start_callback_server(
    expected_state: &str,
    timeout: std::time::Duration,
) -> Result<CallbackResult> {
    let ports_to_try: Vec<u16> = std::iter::once(OAUTH_CALLBACK_PORT)
        .chain(FALLBACK_PORTS.iter().copied())
        .collect();

    let mut listener = None;
    let mut bound_port = 0u16;

    for port in &ports_to_try {
        match TcpListener::bind(("127.0.0.1", *port)).await {
            Ok(l) => {
                bound_port = *port;
                if *port != OAUTH_CALLBACK_PORT {
                    tracing::warn!(
                        primary = OAUTH_CALLBACK_PORT,
                        fallback = port,
                        "Primary OAuth port unavailable, using fallback"
                    );
                } else {
                    tracing::info!(port = port, "OAuth callback server listening");
                }
                listener = Some(l);
                break;
            }
            Err(e) => {
                tracing::warn!(port = port, error = %e, "Failed to bind OAuth callback port");
            }
        }
    }

    let listener = listener.context(format!(
        "Failed to bind OAuth callback server on any port (tried: {:?})",
        ports_to_try
    ))?;

    run_callback_server(listener, expected_state, timeout, bound_port).await
}

/// Core server loop: accepts connections on a pre-bound listener.
async fn run_callback_server(
    listener: TcpListener,
    expected_state: &str,
    timeout: std::time::Duration,
    bound_port: u16,
) -> Result<CallbackResult> {
    let expected_state = expected_state.to_string();
    let (tx, rx) = oneshot::channel::<String>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

    let server_handle = tokio::spawn(async move {
        loop {
            let (stream, _addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to accept connection");
                    continue;
                }
            };

            let tx = Arc::clone(&tx);
            let expected_state = expected_state.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, &expected_state, tx).await {
                    tracing::debug!(error = %e, "Error handling OAuth callback connection");
                }
            });
        }
    });

    // Wait for the code or timeout
    let code = tokio::select! {
        result = rx => {
            result.context("Callback channel closed without receiving code")?
        }
        _ = tokio::time::sleep(timeout) => {
            anyhow::bail!("OAuth callback timeout - no response received within {:?}", timeout);
        }
    };

    server_handle.abort();

    Ok(CallbackResult {
        code,
        port: bound_port,
    })
}

/// Handles a single HTTP connection on the callback server.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    expected_state: &str,
    tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the request line to get the path
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");

    if !path.starts_with("/oauth-callback") {
        let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot found";
        stream.write_all(response.as_bytes()).await?;
        return Ok(());
    }

    // Parse query parameters
    let query = path.split('?').nth(1).unwrap_or("");
    let params: std::collections::HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            Some((parts.next()?, parts.next().unwrap_or("")))
        })
        .collect();

    let error = params.get("error").copied();
    let code = params.get("code").copied();
    let state = params.get("state").copied();

    if let Some(err) = error {
        let body = format!(
            "<html><body><h1>Authentication Failed</h1><p>Error: {}</p></body></html>",
            err
        );
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        anyhow::bail!("OAuth error: {}", err);
    }

    if state != Some(expected_state) {
        let body = "<html><body><h1>Authentication Failed</h1><p>State mismatch.</p></body></html>";
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        anyhow::bail!("State mismatch");
    }

    let code = match code {
        Some(c) if !c.is_empty() => c,
        _ => {
            let body = "<html><body><h1>Authentication Failed</h1><p>No authorization code.</p></body></html>";
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await?;
            anyhow::bail!("No authorization code");
        }
    };

    // Success
    let body = "<html><body><h1>Authentication Successful!</h1><p>You can close this window.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;

    // Send the code through the channel
    if let Some(sender) = tx.lock().await.take() {
        let _ = sender.send(code.to_string());
    }

    Ok(())
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    /// Bind to port 0 (OS-assigned) and return (listener, port).
    async fn bind_ephemeral() -> (TcpListener, u16) {
        let listener = TcpListener::bind(("127.0.0.1", 0u16))
            .await
            .expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    #[tokio::test]
    async fn test_callback_server_timeout() {
        let (listener, _port) = bind_ephemeral().await;
        let result = run_callback_server(
            listener,
            "test-state",
            std::time::Duration::from_millis(50),
            0,
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timeout"));
    }

    #[tokio::test]
    async fn test_callback_server_receives_code() {
        let (listener, port) = bind_ephemeral().await;

        let server = tokio::spawn(async move {
            run_callback_server(
                listener,
                "test-state-123",
                std::time::Duration::from_secs(5),
                port,
            )
            .await
        });

        // Give server time to start accepting
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let client = reqwest::Client::new();
        let _ = client
            .get(format!(
                "http://127.0.0.1:{}/oauth-callback?code=test-code-abc&state=test-state-123",
                port
            ))
            .send()
            .await;

        let result = server.await.unwrap();
        assert!(result.is_ok());
        let callback = result.unwrap();
        assert_eq!(callback.code, "test-code-abc");
        assert_eq!(callback.port, port);
    }

    #[tokio::test]
    async fn test_callback_server_rejects_bad_state() {
        let (listener, port) = bind_ephemeral().await;

        let server = tokio::spawn(async move {
            run_callback_server(
                listener,
                "correct-state",
                std::time::Duration::from_secs(2),
                port,
            )
            .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!(
                "http://127.0.0.1:{}/oauth-callback?code=test&state=wrong-state",
                port
            ))
            .send()
            .await;

        if let Ok(r) = resp {
            assert_eq!(r.status().as_u16(), 400);
        }

        // Server should timeout since no valid code was received
        let result = server.await.unwrap();
        assert!(result.is_err());
    }
}
