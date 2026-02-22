//! Cloud Code API calls for project discovery and user onboarding.
//!
//! Implements loadCodeAssist and onboardUser with endpoint failover,
//! matching the antigravity-claude-proxy reference implementation.

use anyhow::{Context, Result};

use super::constants::{
    client_metadata, LOAD_CODE_ASSIST_ENDPOINTS, X_CLIENT_NAME, X_CLIENT_VERSION, X_GOOG_API_CLIENT,
};

/// Discovers the user's Cloud Code project via loadCodeAssist.
///
/// Tries each endpoint in `LOAD_CODE_ASSIST_ENDPOINTS` (prod first).
/// Returns the project ID if found, or `None` if the user has no project.
pub async fn load_code_assist(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<Option<String>> {
    let meta = client_metadata();
    let body = serde_json::json!({
        "metadata": {
            "ideType": meta.ide_type,
            "platform": meta.platform,
            "pluginType": meta.plugin_type,
        }
    });

    let mut last_err = None;

    for endpoint in LOAD_CODE_ASSIST_ENDPOINTS {
        let url = format!("{}/v1internal:loadCodeAssist", endpoint);

        let result = client
            .post(&url)
            .bearer_auth(access_token)
            .header("x-goog-api-client", X_GOOG_API_CLIENT)
            .header("x-client-name", X_CLIENT_NAME)
            .header("x-client-version", X_CLIENT_VERSION)
            .json(&body)
            .send()
            .await;

        let response = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(endpoint = endpoint, error = %e, "loadCodeAssist request failed");
                last_err = Some(anyhow::anyhow!(e));
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            tracing::warn!(
                endpoint = endpoint,
                status = status,
                "loadCodeAssist HTTP error"
            );
            last_err = Some(anyhow::anyhow!("HTTP {}: {}", status, text));
            continue;
        }

        let json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse loadCodeAssist response")?;

        // Extract project ID — can be string or {id: "..."}
        let project = &json["cloudaicompanionProject"];
        if let Some(id) = project.as_str() {
            return Ok(Some(id.to_string()));
        }
        if let Some(id) = project.get("id").and_then(|v| v.as_str()) {
            return Ok(Some(id.to_string()));
        }

        // Response succeeded but no project found
        return Ok(None);
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No endpoints configured for loadCodeAssist")))
}

/// Onboards a new user and provisions a managed Cloud Code project.
///
/// Polls until `done: true` or max retries exceeded.
/// Returns the managed project ID.
pub async fn onboard_user(
    client: &reqwest::Client,
    access_token: &str,
    project_id: Option<&str>,
) -> Result<String> {
    let meta = client_metadata();
    let mut metadata = serde_json::json!({
        "ideType": meta.ide_type,
        "platform": meta.platform,
        "pluginType": meta.plugin_type,
    });
    if let Some(pid) = project_id {
        metadata["duetProject"] = serde_json::Value::String(pid.to_string());
    }

    let body = serde_json::json!({
        "tierId": "free-tier",
        "metadata": metadata,
    });

    let max_attempts = 10;
    let poll_delay = std::time::Duration::from_secs(5);

    for attempt in 1..=max_attempts {
        // Try each endpoint
        for endpoint in LOAD_CODE_ASSIST_ENDPOINTS {
            let url = format!("{}/v1internal:onboardUser", endpoint);

            let result = client
                .post(&url)
                .bearer_auth(access_token)
                .header("x-goog-api-client", X_GOOG_API_CLIENT)
                .header("x-client-name", X_CLIENT_NAME)
                .header("x-client-version", X_CLIENT_VERSION)
                .json(&body)
                .send()
                .await;

            let response = match result {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => {
                    tracing::warn!(
                        endpoint = endpoint,
                        status = r.status().as_u16(),
                        "onboardUser HTTP error"
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(endpoint = endpoint, error = %e, "onboardUser request failed");
                    continue;
                }
            };

            let json: serde_json::Value = response
                .json()
                .await
                .context("Failed to parse onboardUser response")?;

            if json["done"].as_bool() == Some(true) {
                // Extract managed project ID
                if let Some(id) = json["response"]["cloudaicompanionProject"]["id"].as_str() {
                    return Ok(id.to_string());
                }
                // Fall back to the project_id we already have
                if let Some(pid) = project_id {
                    return Ok(pid.to_string());
                }
                anyhow::bail!("onboardUser completed but no project ID in response");
            }

            // Not done yet — break out of endpoint loop to poll again
            tracing::info!(attempt = attempt, "onboardUser not done yet, polling...");
            break;
        }

        if attempt < max_attempts {
            tokio::time::sleep(poll_delay).await;
        }
    }

    anyhow::bail!(
        "onboardUser did not complete after {} attempts",
        max_attempts
    )
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_json_shape() {
        let meta = client_metadata();
        let body = serde_json::json!({
            "metadata": {
                "ideType": meta.ide_type,
                "platform": meta.platform,
                "pluginType": meta.plugin_type,
            }
        });
        assert!(body["metadata"]["ideType"].is_number());
        assert!(body["metadata"]["platform"].is_number());
        assert!(body["metadata"]["pluginType"].is_number());
    }

    #[test]
    fn test_extract_project_id_string() {
        let json: serde_json::Value =
            serde_json::json!({"cloudaicompanionProject": "my-project-123"});
        let project = &json["cloudaicompanionProject"];
        assert_eq!(project.as_str(), Some("my-project-123"));
    }

    #[test]
    fn test_extract_project_id_object() {
        let json: serde_json::Value =
            serde_json::json!({"cloudaicompanionProject": {"id": "my-project-456"}});
        let project = &json["cloudaicompanionProject"];
        assert_eq!(
            project.get("id").and_then(|v| v.as_str()),
            Some("my-project-456")
        );
    }

    #[test]
    fn test_extract_project_id_missing() {
        let json: serde_json::Value = serde_json::json!({});
        let project = &json["cloudaicompanionProject"];
        assert!(project.as_str().is_none());
        assert!(project.get("id").is_none());
    }

    #[test]
    fn test_onboard_body_without_project() {
        let meta = client_metadata();
        let metadata = serde_json::json!({
            "ideType": meta.ide_type,
            "platform": meta.platform,
            "pluginType": meta.plugin_type,
        });
        let body = serde_json::json!({
            "tierId": "free-tier",
            "metadata": metadata,
        });
        assert_eq!(body["tierId"], "free-tier");
        assert!(body["metadata"]["duetProject"].is_null());
    }

    #[test]
    fn test_onboard_body_with_project() {
        let meta = client_metadata();
        let mut metadata = serde_json::json!({
            "ideType": meta.ide_type,
            "platform": meta.platform,
            "pluginType": meta.plugin_type,
        });
        metadata["duetProject"] = serde_json::Value::String("proj-123".into());
        let body = serde_json::json!({
            "tierId": "free-tier",
            "metadata": metadata,
        });
        assert_eq!(body["metadata"]["duetProject"], "proj-123");
    }

    #[test]
    fn test_onboard_response_done_with_project() {
        let json: serde_json::Value = serde_json::json!({
            "done": true,
            "response": {
                "cloudaicompanionProject": {
                    "id": "managed-proj-789"
                }
            }
        });
        assert_eq!(json["done"].as_bool(), Some(true));
        assert_eq!(
            json["response"]["cloudaicompanionProject"]["id"].as_str(),
            Some("managed-proj-789")
        );
    }

    #[test]
    fn test_onboard_response_not_done() {
        let json: serde_json::Value = serde_json::json!({"done": false});
        assert_eq!(json["done"].as_bool(), Some(false));
    }
}
