//! Cloud Code request builder.
//!
//! Wraps a Google Generative AI request in the Cloud Code envelope format
//! and builds the required HTTP headers.

use anyhow::Context;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use uuid::Uuid;

use super::constants::{
    get_model_family, is_thinking_model, ModelFamily, X_CLIENT_NAME, X_CLIENT_VERSION,
    X_GOOG_API_CLIENT,
};

// === Request Envelope ===

/// Builds the Cloud Code request envelope.
///
/// Wraps a Google Generative AI request body in the envelope expected
/// by the Cloud Code `generateContent` / `streamGenerateContent` endpoints.
///
/// # Envelope format
/// ```json
/// {
///   "project": "<projectId>",
///   "model": "<modelName>",
///   "request": "<googleGenAIRequest>",
///   "userAgent": "antigravity",
///   "requestType": "agent",
///   "requestId": "agent-<uuid>"
/// }
/// ```
pub fn build_cloud_code_request(project_id: &str, model: &str, google_request: Value) -> Value {
    let request_id = format!("agent-{}", Uuid::new_v4());
    json!({
        "project": project_id,
        "model": model,
        "request": google_request,
        "userAgent": "antigravity",
        "requestType": "agent",
        "requestId": request_id
    })
}

// === Headers ===

/// Builds the HTTP headers required for Cloud Code API requests.
///
/// Always includes:
/// - `Authorization: Bearer {token}`
/// - `Content-Type: application/json`
/// - `X-Client-Name: antigravity`
/// - `X-Client-Version: 1.107.0`
/// - `x-goog-api-client: gl-node/18.18.2 fire/0.8.6 grpc/1.10.x`
/// - `X-Machine-Session-Id: {session_id}`
///
/// Conditionally includes:
/// - `anthropic-beta: interleaved-thinking-2025-05-14` (only for Claude thinking models)
pub fn build_headers(token: &str, session_id: &str, model: &str) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();

    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .context("Invalid token: contains non-visible ASCII characters")?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("X-Client-Name", HeaderValue::from_static(X_CLIENT_NAME));
    headers.insert(
        "X-Client-Version",
        HeaderValue::from_static(X_CLIENT_VERSION),
    );
    headers.insert(
        "x-goog-api-client",
        HeaderValue::from_static(X_GOOG_API_CLIENT),
    );
    headers.insert(
        "X-Machine-Session-Id",
        HeaderValue::from_str(session_id)
            .context("Invalid session ID: contains non-visible ASCII characters")?,
    );

    // Add anthropic-beta header for Claude thinking models
    if get_model_family(model) == ModelFamily::Claude && is_thinking_model(model) {
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static("interleaved-thinking-2025-05-14"),
        );
    }

    Ok(headers)
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cloud_code_request_structure() {
        let google_req = json!({"contents": [], "generationConfig": {}});
        let envelope = build_cloud_code_request("my-project", "gemini-3-flash", google_req);

        assert_eq!(envelope["project"], "my-project");
        assert_eq!(envelope["model"], "gemini-3-flash");
        assert_eq!(envelope["userAgent"], "antigravity");
        assert_eq!(envelope["requestType"], "agent");
        assert!(envelope["request"].is_object());
        let rid = envelope["requestId"].as_str().unwrap();
        assert!(
            rid.starts_with("agent-"),
            "requestId should start with agent-"
        );
    }

    #[test]
    fn test_build_cloud_code_request_unique_ids() {
        let req = json!({});
        let e1 = build_cloud_code_request("p", "m", req.clone());
        let e2 = build_cloud_code_request("p", "m", req);
        assert_ne!(e1["requestId"], e2["requestId"]);
    }

    #[test]
    fn test_build_cloud_code_request_preserves_google_request() {
        let google_req =
            json!({"contents": [{"role": "user"}], "generationConfig": {"maxOutputTokens": 1024}});
        let envelope = build_cloud_code_request("proj", "model", google_req.clone());
        assert_eq!(envelope["request"], google_req);
    }

    #[test]
    fn test_build_headers_basic() {
        let headers = build_headers("my-token", "session-123", "gemini-3-flash").unwrap();

        assert_eq!(
            headers.get(AUTHORIZATION).unwrap().to_str().unwrap(),
            "Bearer my-token"
        );
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap().to_str().unwrap(),
            "application/json"
        );
        assert_eq!(
            headers.get("X-Client-Name").unwrap().to_str().unwrap(),
            "antigravity"
        );
        assert_eq!(
            headers.get("X-Client-Version").unwrap().to_str().unwrap(),
            X_CLIENT_VERSION
        );
        assert_eq!(
            headers.get("x-goog-api-client").unwrap().to_str().unwrap(),
            X_GOOG_API_CLIENT
        );
        assert_eq!(
            headers
                .get("X-Machine-Session-Id")
                .unwrap()
                .to_str()
                .unwrap(),
            "session-123"
        );
    }

    #[test]
    fn test_build_headers_no_anthropic_beta_for_gemini() {
        let headers = build_headers("tok", "sess", "gemini-3-flash").unwrap();
        assert!(headers.get("anthropic-beta").is_none());
    }

    #[test]
    fn test_build_headers_no_anthropic_beta_for_non_thinking_claude() {
        let headers = build_headers("tok", "sess", "claude-sonnet-4-5").unwrap();
        assert!(headers.get("anthropic-beta").is_none());
    }

    #[test]
    fn test_build_headers_anthropic_beta_for_claude_thinking() {
        let headers = build_headers("tok", "sess", "claude-sonnet-4-5-thinking").unwrap();
        assert_eq!(
            headers.get("anthropic-beta").unwrap().to_str().unwrap(),
            "interleaved-thinking-2025-05-14"
        );
    }

    #[test]
    fn test_build_headers_anthropic_beta_for_claude_opus_thinking() {
        let headers = build_headers("tok", "sess", "claude-opus-4-6-thinking").unwrap();
        assert!(headers.get("anthropic-beta").is_some());
    }
}
