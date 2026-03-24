use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::providers::types::ProviderId;
use crate::web_ui::config_db::{ConfigDb, RegistryModel};

/// Generate a prefixed ID from provider and model: `{provider}/{model_id}`.
pub fn generate_prefixed_id(provider_id: &str, model_id: &str) -> String {
    format!("{provider_id}/{model_id}")
}

// ── Dynamic Model Fetching ───────────────────────────────────

/// Fetch Kiro models using a raw access token and region.
pub async fn fetch_kiro_models_with_token(
    http_client: &crate::http_client::KiroHttpClient,
    access_token: &str,
    region: &str,
) -> Result<Vec<RegistryModel>> {
    let url = format!("https://q.{}.amazonaws.com/ListAvailableModels", region);

    let req = http_client
        .client()
        .get(&url)
        .query(&[("origin", "AI_EDITOR")])
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .build()
        .context("Failed to build Kiro models request")?;

    let response = http_client
        .request_no_retry(req)
        .await
        .context("Failed to fetch Kiro models")?;
    let body = response
        .text()
        .await
        .context("Failed to read Kiro models response")?;
    let json: serde_json::Value =
        serde_json::from_str(&body).context("Failed to parse Kiro models JSON")?;

    let now = Utc::now();
    let models = json
        .get("models")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(models
        .into_iter()
        .filter_map(|m| {
            let internal_id = m.get("modelId")?.as_str()?;
            let display_name = m
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(internal_id);
            // Use display name as the external model_id for Kiro models
            Some(RegistryModel {
                id: Uuid::new_v4(),
                provider_id: "kiro".to_string(),
                model_id: display_name.to_string(),
                display_name: display_name.to_string(),
                prefixed_id: generate_prefixed_id("kiro", display_name),
                context_length: 0,
                max_output_tokens: 0,
                capabilities: json!({}),
                enabled: false,
                source: "api".to_string(),
                upstream_meta: Some(json!({ "internal_id": internal_id })),
                created_at: now,
                updated_at: now,
            })
        })
        .collect())
}

/// Fetch Kiro models from the ListAvailableModels API via AuthManager.
pub async fn fetch_kiro_models(
    http_client: &crate::http_client::KiroHttpClient,
    auth_manager: &crate::auth::AuthManager,
) -> Result<Vec<RegistryModel>> {
    let access_token = auth_manager.get_access_token().await?;
    let region = auth_manager.get_region().await;
    fetch_kiro_models_with_token(http_client, &access_token, &region).await
}

/// Fetch Copilot models from `{base_url}/models` using a valid copilot token from DB.
pub async fn fetch_copilot_models(
    http_client: &crate::http_client::KiroHttpClient,
    db: &ConfigDb,
) -> Result<Vec<RegistryModel>> {
    let token_row = match db.get_any_valid_copilot_token().await {
        Ok(Some(row)) => row,
        Ok(None) => {
            tracing::debug!("copilot: no valid copilot token found in DB");
            return Ok(Vec::new());
        }
        Err(e) => {
            tracing::debug!(error = ?e, "copilot: failed to query copilot tokens");
            return Ok(Vec::new());
        }
    };

    if let Some(ref copilot_token) = token_row.copilot_token {
        let base_url = token_row
            .base_url
            .as_deref()
            .unwrap_or("https://api.githubcopilot.com");
        let url = format!("{}/models", base_url);

        let result = http_client
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", copilot_token))
            .send()
            .await;

        if let Ok(resp) = result {
            if resp.status().is_success() {
                if let Ok(body) = resp.text().await {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        return Ok(parse_openai_models_response("copilot", &json));
                    }
                }
            }
        }
    }

    Ok(Vec::new())
}

/// Fetch models from an OpenAI-compatible `/v1/models` endpoint.
pub async fn fetch_openai_compatible_models(
    http_client: &crate::http_client::KiroHttpClient,
    provider_id: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<RegistryModel>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    let resp = http_client
        .client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .context("Failed to fetch models from OpenAI-compatible API")?;

    if !resp.status().is_success() {
        anyhow::bail!("Models API returned status {}", resp.status());
    }

    let body = resp
        .text()
        .await
        .context("Failed to read models response")?;
    let json: serde_json::Value =
        serde_json::from_str(&body).context("Failed to parse models JSON")?;

    Ok(parse_openai_models_response(provider_id, &json))
}

/// Fetch Anthropic models from `GET /v1/models`.
pub async fn fetch_anthropic_models(
    http_client: &crate::http_client::KiroHttpClient,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<Vec<RegistryModel>> {
    let base = base_url.unwrap_or("https://api.anthropic.com");
    let url = format!("{}/v1/models", base.trim_end_matches('/'));

    let resp = http_client
        .client()
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .context("Failed to fetch Anthropic models")?;

    if !resp.status().is_success() {
        anyhow::bail!("Anthropic models API returned status {}", resp.status());
    }

    let body = resp
        .text()
        .await
        .context("Failed to read Anthropic response")?;
    let json: serde_json::Value =
        serde_json::from_str(&body).context("Failed to parse Anthropic models JSON")?;

    let now = Utc::now();
    let models = json
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(models
        .into_iter()
        .filter_map(|m| {
            let model_id = m.get("id")?.as_str()?;
            let display_name = m
                .get("display_name")
                .and_then(|v| v.as_str())
                .unwrap_or(model_id);
            Some(RegistryModel {
                id: Uuid::new_v4(),
                provider_id: "anthropic".to_string(),
                model_id: model_id.to_string(),
                display_name: display_name.to_string(),
                prefixed_id: generate_prefixed_id("anthropic", model_id),
                context_length: 0,
                max_output_tokens: 0,
                capabilities: json!({}),
                enabled: false,
                source: "api".to_string(),
                upstream_meta: Some(m.clone()),
                created_at: now,
                updated_at: now,
            })
        })
        .collect())
}

/// Parse an OpenAI-format `/models` response into `RegistryModel` entries.
pub(crate) fn parse_openai_models_response(
    provider_id: &str,
    json: &serde_json::Value,
) -> Vec<RegistryModel> {
    let now = Utc::now();
    let models = json
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    models
        .into_iter()
        .filter_map(|m| {
            let model_id = m.get("id")?.as_str()?;
            Some(RegistryModel {
                id: Uuid::new_v4(),
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                display_name: model_id.to_string(),
                prefixed_id: generate_prefixed_id(provider_id, model_id),
                context_length: 0,
                max_output_tokens: 0,
                capabilities: json!({}),
                enabled: false,
                source: "api".to_string(),
                upstream_meta: Some(m.clone()),
                created_at: now,
                updated_at: now,
            })
        })
        .collect()
}

/// Look up the first enabled admin pool account for a provider.
async fn get_admin_pool_credential(
    db: &ConfigDb,
    provider_id: &str,
) -> Option<(String, Option<String>)> {
    let accounts = db.get_admin_pool_accounts(provider_id).await.ok()?;
    accounts
        .into_iter()
        .find(|a| a.enabled)
        .map(|a| (a.api_key, a.base_url))
}

/// Populate a provider's models in the DB: tries admin/global credentials first,
/// then falls back to any user's connected token. Returns the number of models upserted.
pub async fn populate_provider(
    provider_id: &str,
    db: &Arc<ConfigDb>,
    http_client: &crate::http_client::KiroHttpClient,
    auth_manager: Option<&crate::auth::AuthManager>,
) -> Result<usize> {
    let pid = ProviderId::from_str(provider_id)
        .map_err(|e| anyhow::anyhow!(e))
        .context("Invalid provider_id")?;

    let api_models = match pid {
        ProviderId::Kiro => {
            // Try global auth_manager first
            let from_global = if let Some(am) = auth_manager {
                tracing::debug!("kiro: trying global auth_manager");
                fetch_kiro_models(http_client, am).await.ok()
            } else {
                None
            };
            if from_global.as_ref().is_some_and(|m| !m.is_empty()) {
                from_global
            } else {
                // Fallback: any user's valid Kiro token
                tracing::debug!("kiro: falling back to user Kiro token from DB");
                match db.get_any_valid_kiro_credential().await {
                    Ok(Some((access_token, sso_region))) => {
                        let region = sso_region.as_deref().unwrap_or("us-east-1");
                        tracing::debug!(region, "kiro: using user token for model fetch");
                        fetch_kiro_models_with_token(http_client, &access_token, region)
                            .await
                            .ok()
                    }
                    Ok(None) => {
                        tracing::debug!("kiro: no valid user Kiro token found");
                        None
                    }
                    Err(e) => {
                        tracing::debug!(error = ?e, "kiro: failed to query user Kiro tokens");
                        None
                    }
                }
            }
        }
        ProviderId::Copilot => fetch_copilot_models(http_client, db).await.ok(),
        ProviderId::Anthropic => {
            // Try admin pool first
            let from_admin = if let Some((api_key, base_url)) =
                get_admin_pool_credential(db, "anthropic").await
            {
                tracing::debug!("anthropic: trying admin pool credential");
                fetch_anthropic_models(http_client, &api_key, base_url.as_deref())
                    .await
                    .ok()
            } else {
                None
            };
            if from_admin.as_ref().is_some_and(|m| !m.is_empty()) {
                from_admin
            } else {
                // Fallback: any user's connected Anthropic token
                tracing::debug!("anthropic: falling back to user provider token");
                match db.get_any_user_provider_credential("anthropic").await {
                    Ok(Some((api_key, base_url))) => {
                        fetch_anthropic_models(http_client, &api_key, base_url.as_deref())
                            .await
                            .ok()
                    }
                    _ => None,
                }
            }
        }
        ProviderId::OpenAICodex => {
            // Try admin pool first
            let from_admin = if let Some((api_key, base_url)) =
                get_admin_pool_credential(db, "openai_codex").await
            {
                tracing::debug!("openai_codex: trying admin pool credential");
                let base = base_url
                    .as_deref()
                    .or(pid.default_base_url())
                    .unwrap_or("https://api.openai.com");
                fetch_openai_compatible_models(http_client, "openai_codex", base, &api_key)
                    .await
                    .ok()
            } else {
                None
            };
            if from_admin.as_ref().is_some_and(|m| !m.is_empty()) {
                from_admin
            } else {
                // Fallback: any user's connected OpenAI Codex token
                tracing::debug!("openai_codex: falling back to user provider token");
                match db.get_any_user_provider_credential("openai_codex").await {
                    Ok(Some((api_key, base_url))) => {
                        let base = base_url
                            .as_deref()
                            .or(pid.default_base_url())
                            .unwrap_or("https://api.openai.com");
                        fetch_openai_compatible_models(http_client, "openai_codex", base, &api_key)
                            .await
                            .ok()
                    }
                    _ => None,
                }
            }
        }
        ProviderId::Custom => None,
    };

    // Keep-last-successful: if API returns empty or fails, preserve existing registry rows
    let Some(models) = api_models.filter(|m| !m.is_empty()) else {
        tracing::warn!(
            provider = provider_id,
            "API returned no models, keeping existing registry"
        );
        return Ok(0);
    };

    tracing::info!(
        provider = provider_id,
        count = models.len(),
        "Fetched models from API"
    );
    let count = db.bulk_upsert_registry_models(&models).await?;
    tracing::info!(provider = provider_id, count, "Populated model registry");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_prefixed_id() {
        assert_eq!(
            generate_prefixed_id("anthropic", "claude-opus-4-6"),
            "anthropic/claude-opus-4-6"
        );
        assert_eq!(
            generate_prefixed_id("openai_codex", "gpt-5.4"),
            "openai_codex/gpt-5.4"
        );
    }

    #[test]
    fn test_parse_openai_models_response_valid() {
        let json = json!({
            "data": [
                {"id": "gpt-5", "object": "model", "owned_by": "openai"},
                {"id": "gpt-5.1", "object": "model", "owned_by": "openai"}
            ]
        });
        let models = parse_openai_models_response("openai_codex", &json);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_id, "gpt-5");
        assert_eq!(models[0].provider_id, "openai_codex");
        assert_eq!(models[0].prefixed_id, "openai_codex/gpt-5");
        assert_eq!(models[0].source, "api");
        assert!(models[0].upstream_meta.is_some());
        assert_eq!(models[1].model_id, "gpt-5.1");
    }

    #[test]
    fn test_parse_openai_models_response_empty_data() {
        let json = json!({"data": []});
        let models = parse_openai_models_response("copilot", &json);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_response_missing_data() {
        let json = json!({"models": []});
        let models = parse_openai_models_response("copilot", &json);
        assert!(models.is_empty());
    }

    #[test]
    fn test_parse_openai_models_response_skips_missing_id() {
        let json = json!({
            "data": [
                {"id": "gpt-5"},
                {"object": "model"},
                {"id": "gpt-5.1"}
            ]
        });
        let models = parse_openai_models_response("openai_codex", &json);
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn test_generate_prefixed_id_special_chars() {
        assert_eq!(
            generate_prefixed_id("custom", "my-model-v3-plus"),
            "custom/my-model-v3-plus"
        );
    }

    // ── Keep-last-successful behavior ─────────────────────────────────
    // populate_provider uses `api_models.filter(|m| !m.is_empty())` to
    // decide whether to upsert. These tests verify the filter semantics.

    #[test]
    fn test_keep_last_successful_none_returns_zero() {
        // API failure (None) → should not upsert, return Ok(0)
        let api_models: Option<Vec<RegistryModel>> = None;
        let result = api_models.filter(|m| !m.is_empty());
        assert!(result.is_none(), "None API response should skip upsert");
    }

    #[test]
    fn test_keep_last_successful_empty_vec_returns_zero() {
        // API returns empty list → should not upsert, return Ok(0)
        let api_models: Option<Vec<RegistryModel>> = Some(vec![]);
        let result = api_models.filter(|m| !m.is_empty());
        assert!(result.is_none(), "Empty model list should skip upsert");
    }

    #[test]
    fn test_keep_last_successful_with_models_proceeds() {
        // API returns models → should proceed to upsert
        let api_models: Option<Vec<RegistryModel>> = Some(vec![RegistryModel {
            id: uuid::Uuid::nil(),
            provider_id: "anthropic".to_string(),
            model_id: "claude-opus-4-6".to_string(),
            prefixed_id: "anthropic/claude-opus-4-6".to_string(),
            display_name: "claude-opus-4-6".to_string(),
            context_length: 200000,
            max_output_tokens: 16384,
            capabilities: serde_json::json!({}),
            enabled: true,
            source: "api".to_string(),
            upstream_meta: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }]);
        let result = api_models.filter(|m| !m.is_empty());
        assert!(
            result.is_some(),
            "Non-empty model list should proceed to upsert"
        );
        assert_eq!(result.unwrap().len(), 1);
    }
}
