use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::web_ui::config_db::{ConfigDb, RegistryModel};

/// A static model definition before it gets a UUID and timestamps.
struct StaticModel {
    model_id: &'static str,
    display_name: &'static str,
    context_length: i32,
    max_output_tokens: i32,
    capabilities: serde_json::Value,
}

/// Generate a prefixed ID from provider and model: `{provider}/{model_id}`.
pub fn generate_prefixed_id(provider_id: &str, model_id: &str) -> String {
    format!("{provider_id}/{model_id}")
}

/// Convert static definitions into `RegistryModel` instances for a given provider.
fn static_to_registry(provider_id: &str, models: Vec<StaticModel>) -> Vec<RegistryModel> {
    let now = Utc::now();
    models
        .into_iter()
        .map(|m| RegistryModel {
            id: Uuid::new_v4(),
            provider_id: provider_id.to_string(),
            model_id: m.model_id.to_string(),
            display_name: m.display_name.to_string(),
            prefixed_id: generate_prefixed_id(provider_id, m.model_id),
            context_length: m.context_length,
            max_output_tokens: m.max_output_tokens,
            capabilities: m.capabilities,
            enabled: false,
            source: "static".to_string(),
            upstream_meta: None,
            created_at: now,
            updated_at: now,
        })
        .collect()
}

/// Static Anthropic (Claude) model definitions.
pub fn anthropic_static_models() -> Vec<RegistryModel> {
    static_to_registry(
        "anthropic",
        vec![
            StaticModel {
                model_id: "claude-opus-4-6",
                display_name: "Claude 4.6 Opus",
                context_length: 1_000_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-sonnet-4-6",
                display_name: "Claude 4.6 Sonnet",
                context_length: 200_000,
                max_output_tokens: 64_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-haiku-4-5-20251001",
                display_name: "Claude 4.5 Haiku",
                context_length: 200_000,
                max_output_tokens: 64_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-sonnet-4-5-20250929",
                display_name: "Claude 4.5 Sonnet",
                context_length: 200_000,
                max_output_tokens: 64_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-opus-4-5-20251101",
                display_name: "Claude 4.5 Opus",
                context_length: 200_000,
                max_output_tokens: 64_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-opus-4-1-20250805",
                display_name: "Claude 4.1 Opus",
                context_length: 200_000,
                max_output_tokens: 32_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-opus-4-20250514",
                display_name: "Claude 4 Opus",
                context_length: 200_000,
                max_output_tokens: 32_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-sonnet-4-20250514",
                display_name: "Claude 4 Sonnet",
                context_length: 200_000,
                max_output_tokens: 64_000,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-3-7-sonnet-20250219",
                display_name: "Claude 3.7 Sonnet",
                context_length: 128_000,
                max_output_tokens: 8_192,
                capabilities: json!({"thinking": true, "vision": true}),
            },
            StaticModel {
                model_id: "claude-3-5-haiku-20241022",
                display_name: "Claude 3.5 Haiku",
                context_length: 128_000,
                max_output_tokens: 8_192,
                capabilities: json!({"vision": true}),
            },
        ],
    )
}

/// Static OpenAI Codex model definitions.
pub fn openai_codex_static_models() -> Vec<RegistryModel> {
    static_to_registry(
        "openai_codex",
        vec![
            StaticModel {
                model_id: "gpt-5.4",
                display_name: "GPT 5.4",
                context_length: 1_050_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.3-codex",
                display_name: "GPT 5.3 Codex",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.3-codex-spark",
                display_name: "GPT 5.3 Codex Spark",
                context_length: 128_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.2",
                display_name: "GPT 5.2",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.2-codex",
                display_name: "GPT 5.2 Codex",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.1",
                display_name: "GPT 5.1",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.1-codex",
                display_name: "GPT 5.1 Codex",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.1-codex-mini",
                display_name: "GPT 5.1 Codex Mini",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5.1-codex-max",
                display_name: "GPT 5.1 Codex Max",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5",
                display_name: "GPT 5",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5-codex",
                display_name: "GPT 5 Codex",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
            StaticModel {
                model_id: "gpt-5-codex-mini",
                display_name: "GPT 5 Codex Mini",
                context_length: 400_000,
                max_output_tokens: 128_000,
                capabilities: json!({"thinking": true, "tools": true}),
            },
        ],
    )
}

/// Static Qwen model definitions.
pub fn qwen_static_models() -> Vec<RegistryModel> {
    static_to_registry(
        "qwen",
        vec![
            StaticModel {
                model_id: "coder-model",
                display_name: "Qwen 3.5 Plus",
                context_length: 1_048_576,
                max_output_tokens: 65_536,
                capabilities: json!({}),
            },
            StaticModel {
                model_id: "qwen3-coder-plus",
                display_name: "Qwen3 Coder Plus",
                context_length: 32_768,
                max_output_tokens: 8_192,
                capabilities: json!({}),
            },
            StaticModel {
                model_id: "qwen3-coder-flash",
                display_name: "Qwen3 Coder Flash",
                context_length: 8_192,
                max_output_tokens: 2_048,
                capabilities: json!({}),
            },
            StaticModel {
                model_id: "vision-model",
                display_name: "Qwen3 Vision Model",
                context_length: 32_768,
                max_output_tokens: 2_048,
                capabilities: json!({"vision": true}),
            },
        ],
    )
}

/// Get all static models across all providers.
#[allow(dead_code)]
pub fn all_static_models() -> Vec<RegistryModel> {
    let mut models = Vec::new();
    models.extend(anthropic_static_models());
    models.extend(openai_codex_static_models());
    models.extend(qwen_static_models());
    models
}

// ── Dynamic Model Fetching ───────────────────────────────────

/// Fetch Kiro models from the ListAvailableModels API, storing internal IDs in upstream_meta.
pub async fn fetch_kiro_models(
    http_client: &crate::http_client::KiroHttpClient,
    auth_manager: &crate::auth::AuthManager,
) -> Result<Vec<RegistryModel>> {
    let access_token = auth_manager.get_access_token().await?;
    let region = auth_manager.get_region().await;
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

/// Fetch Copilot models from `{base_url}/models` using a copilot token from DB.
pub async fn fetch_copilot_models(
    http_client: &crate::http_client::KiroHttpClient,
    db: &ConfigDb,
) -> Result<Vec<RegistryModel>> {
    // Find any user with a valid copilot token
    let tokens = db
        .get_expiring_copilot_tokens()
        .await
        .ok()
        .unwrap_or_default();

    // Try all copilot tokens, or fall back to fetching all
    let all_tokens = if tokens.is_empty() {
        // No expiring tokens — try to get any user's token
        Vec::new()
    } else {
        tokens
    };

    for token_row in &all_tokens {
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
    }

    // No valid copilot token available
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

/// Populate a provider's models in the DB: tries API first, falls back to static.
/// Returns the number of models upserted.
pub async fn populate_provider(
    provider_id: &str,
    db: &Arc<ConfigDb>,
    http_client: &crate::http_client::KiroHttpClient,
    auth_manager: Option<&crate::auth::AuthManager>,
) -> Result<usize> {
    let api_models = match provider_id {
        "kiro" => {
            if let Some(am) = auth_manager {
                fetch_kiro_models(http_client, am).await.ok()
            } else {
                None
            }
        }
        "copilot" => fetch_copilot_models(http_client, db).await.ok(),
        "anthropic" => {
            if let Some((api_key, base_url)) = get_admin_pool_credential(db, "anthropic").await {
                fetch_anthropic_models(http_client, &api_key, base_url.as_deref())
                    .await
                    .ok()
            } else {
                None
            }
        }
        "openai_codex" => {
            if let Some((api_key, base_url)) =
                get_admin_pool_credential(db, "openai_codex").await
            {
                let base = base_url.as_deref().unwrap_or("https://api.openai.com");
                fetch_openai_compatible_models(http_client, "openai_codex", base, &api_key)
                    .await
                    .ok()
            } else {
                None
            }
        }
        "qwen" => {
            if let Some((api_key, base_url)) = get_admin_pool_credential(db, "qwen").await {
                let base = base_url.as_deref().unwrap_or("https://chat.qwen.ai/api");
                fetch_openai_compatible_models(http_client, "qwen", base, &api_key)
                    .await
                    .ok()
            } else {
                None
            }
        }
        _ => None,
    };

    let models = if let Some(api) = api_models {
        if api.is_empty() {
            tracing::debug!(
                provider = provider_id,
                "API returned no models, using static fallback"
            );
            get_static_for_provider(provider_id)
        } else {
            tracing::info!(
                provider = provider_id,
                count = api.len(),
                "Fetched models from API"
            );
            api
        }
    } else {
        tracing::debug!(
            provider = provider_id,
            "No API available, using static models"
        );
        get_static_for_provider(provider_id)
    };

    if models.is_empty() {
        return Ok(0);
    }

    let count = db.bulk_upsert_registry_models(&models).await?;
    tracing::info!(provider = provider_id, count, "Populated model registry");
    Ok(count)
}

/// Populate a provider using a user's API key (for anthropic, openai_codex).
#[allow(dead_code)]
pub async fn populate_provider_with_key(
    provider_id: &str,
    api_key: &str,
    db: &Arc<ConfigDb>,
    http_client: &crate::http_client::KiroHttpClient,
) -> Result<usize> {
    let api_models = match provider_id {
        "anthropic" => fetch_anthropic_models(http_client, api_key, None).await.ok(),
        "openai_codex" => fetch_openai_compatible_models(
            http_client,
            "openai_codex",
            "https://api.openai.com",
            api_key,
        )
        .await
        .ok(),
        _ => None,
    };

    let models = if let Some(api) = api_models {
        if api.is_empty() {
            get_static_for_provider(provider_id)
        } else {
            tracing::info!(
                provider = provider_id,
                count = api.len(),
                "Fetched models from API with key"
            );
            api
        }
    } else {
        get_static_for_provider(provider_id)
    };

    if models.is_empty() {
        return Ok(0);
    }

    let count = db.bulk_upsert_registry_models(&models).await?;
    Ok(count)
}

/// Get static models for a specific provider.
fn get_static_for_provider(provider_id: &str) -> Vec<RegistryModel> {
    match provider_id {
        "anthropic" => anthropic_static_models(),
        "openai_codex" => openai_codex_static_models(),
        "qwen" => qwen_static_models(),
        _ => Vec::new(), // kiro, copilot have no static fallback
    }
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
    fn test_anthropic_static_models_not_empty() {
        let models = anthropic_static_models();
        assert!(!models.is_empty());
        assert!(models.len() >= 10);
        for m in &models {
            assert_eq!(m.provider_id, "anthropic");
            assert_eq!(m.source, "static");
            assert!(!m.enabled);
            assert!(m.prefixed_id.starts_with("anthropic/"));
        }
    }

    #[test]
    fn test_openai_codex_static_models_not_empty() {
        let models = openai_codex_static_models();
        assert!(!models.is_empty());
        for m in &models {
            assert_eq!(m.provider_id, "openai_codex");
            assert!(m.prefixed_id.starts_with("openai_codex/"));
        }
    }

    #[test]
    fn test_qwen_static_models_not_empty() {
        let models = qwen_static_models();
        assert!(!models.is_empty());
        for m in &models {
            assert_eq!(m.provider_id, "qwen");
            assert!(m.prefixed_id.starts_with("qwen/"));
        }
    }

    #[test]
    fn test_all_static_models_aggregates() {
        let all = all_static_models();
        let anthropic_count = anthropic_static_models().len();
        let openai_count = openai_codex_static_models().len();
        let qwen_count = qwen_static_models().len();
        assert_eq!(all.len(), anthropic_count + openai_count + qwen_count);
    }

    #[test]
    fn test_prefixed_ids_unique() {
        let all = all_static_models();
        let mut ids: Vec<&str> = all.iter().map(|m| m.prefixed_id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "prefixed_ids must be unique");
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
    fn test_get_static_for_provider_known() {
        assert!(!get_static_for_provider("anthropic").is_empty());
        assert!(!get_static_for_provider("openai_codex").is_empty());
        assert!(!get_static_for_provider("qwen").is_empty());
    }

    #[test]
    fn test_get_static_for_provider_unknown() {
        assert!(get_static_for_provider("kiro").is_empty());
        assert!(get_static_for_provider("copilot").is_empty());
        assert!(get_static_for_provider("gemini").is_empty());
        assert!(get_static_for_provider("nonexistent").is_empty());
    }

    #[test]
    fn test_static_models_have_valid_context_lengths() {
        for m in all_static_models() {
            assert!(
                m.context_length > 0,
                "{}/{} has zero context_length",
                m.provider_id,
                m.model_id
            );
            assert!(
                m.max_output_tokens > 0,
                "{}/{} has zero max_output_tokens",
                m.provider_id,
                m.model_id
            );
        }
    }

    #[test]
    fn test_static_models_model_ids_match_known_patterns() {
        let anthropic = anthropic_static_models();
        for m in &anthropic {
            assert!(
                m.model_id.starts_with("claude-"),
                "Anthropic model_id should start with claude-: {}",
                m.model_id
            );
        }

        let openai = openai_codex_static_models();
        for m in &openai {
            assert!(
                m.model_id.starts_with("gpt-"),
                "OpenAI model_id should start with gpt-: {}",
                m.model_id
            );
        }
    }

    #[test]
    fn test_generate_prefixed_id_special_chars() {
        assert_eq!(
            generate_prefixed_id("qwen", "qwen3-coder-plus"),
            "qwen/qwen3-coder-plus"
        );
    }
}
