use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::ApiError;
use crate::guardrails::db::GuardrailsDb;
use crate::guardrails::types::{ApplyTo, GuardrailProfile, GuardrailRule};
use crate::routes::{AppState, SessionInfo};

// ── Request/Response types ───────────────────────────────────────────

#[derive(Deserialize)]
struct CreateProfileRequest {
    name: String,
    provider_name: Option<String>,
    enabled: Option<bool>,
    guardrail_id: String,
    guardrail_version: Option<String>,
    region: Option<String>,
    access_key: String,
    secret_key: String,
}

#[derive(Deserialize)]
struct UpdateProfileRequest {
    name: String,
    provider_name: Option<String>,
    enabled: Option<bool>,
    guardrail_id: String,
    guardrail_version: Option<String>,
    region: Option<String>,
    access_key: String,
    secret_key: Option<String>,
}

#[derive(Deserialize)]
struct CreateRuleRequest {
    name: String,
    description: Option<String>,
    enabled: Option<bool>,
    cel_expression: Option<String>,
    apply_to: Option<String>,
    sampling_rate: Option<i16>,
    timeout_ms: Option<i32>,
    profile_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
struct UpdateRuleRequest {
    name: String,
    description: Option<String>,
    enabled: Option<bool>,
    cel_expression: Option<String>,
    apply_to: Option<String>,
    sampling_rate: Option<i16>,
    timeout_ms: Option<i32>,
    profile_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
struct TestProfileRequest {
    profile_id: Uuid,
    content: String,
}

#[derive(Serialize)]
struct TestProfileResponse {
    success: bool,
    action: String,
    response_time_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ValidateCelRequest {
    expression: String,
}

#[derive(Serialize)]
struct ValidateCelResponse {
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ── Helper ───────────────────────────────────────────────────────────

/// Mask secret_key: show first 4 and last 4 chars, or "****" if too short.
fn mask_secret(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > 8 {
        let prefix: String = chars[..4].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", prefix, suffix)
    } else if chars.is_empty() {
        String::new()
    } else {
        "****".to_string()
    }
}

/// Serialize a profile to JSON, masking the secret_key.
fn profile_to_json(p: &GuardrailProfile) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "provider_name": p.provider_name,
        "enabled": p.enabled,
        "guardrail_id": p.guardrail_id,
        "guardrail_version": p.guardrail_version,
        "region": p.region,
        "access_key": mask_secret(&p.access_key),
        "secret_key": mask_secret(&p.secret_key),
        "created_at": p.created_at.to_rfc3339(),
        "updated_at": p.updated_at.to_rfc3339(),
    })
}

/// Serialize a rule to JSON.
fn rule_to_json(r: &GuardrailRule) -> Value {
    json!({
        "id": r.id,
        "name": r.name,
        "description": r.description,
        "enabled": r.enabled,
        "cel_expression": r.cel_expression,
        "apply_to": r.apply_to,
        "sampling_rate": r.sampling_rate,
        "timeout_ms": r.timeout_ms,
        "profile_ids": r.profile_ids,
        "created_at": r.created_at.to_rfc3339(),
        "updated_at": r.updated_at.to_rfc3339(),
    })
}

/// Construct a GuardrailsDb from the config_db pool.
///
/// The guardrails tables share the same PostgreSQL database as the config tables.
fn require_guardrails_db(state: &AppState) -> Result<GuardrailsDb, ApiError> {
    let config_db = state.require_config_db()?;
    Ok(GuardrailsDb::new(config_db.pool().clone()))
}

/// Reload the guardrails engine config after a CRUD mutation.
async fn reload_engine(state: &AppState) {
    if let Some(ref engine) = state.guardrails_engine {
        let config_db = match state.require_config_db() {
            Ok(db) => db,
            Err(_) => return,
        };
        let guardrails_db = GuardrailsDb::new(config_db.pool().clone());
        let enabled = state
            .config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .guardrails_enabled;
        if let Err(e) = engine.reload(&guardrails_db, enabled).await {
            tracing::error!(error = %e, "Failed to reload guardrails engine config");
        }
    }
}

// ── Profile handlers ─────────────────────────────────────────────────

/// GET /guardrails/profiles — List all profiles (secret_key masked)
async fn list_profiles(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
) -> Result<Json<Value>, ApiError> {
    let db = require_guardrails_db(&state)?;
    let profiles = db.list_profiles().await.map_err(ApiError::Internal)?;

    let items: Vec<Value> = profiles.iter().map(profile_to_json).collect();
    Ok(Json(json!({ "profiles": items, "count": items.len() })))
}

/// POST /guardrails/profiles — Create profile
async fn create_profile(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Json(body): Json<CreateProfileRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "Profile name cannot be empty".to_string(),
        ));
    }
    if body.guardrail_id.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "guardrail_id cannot be empty".to_string(),
        ));
    }

    let db = require_guardrails_db(&state)?;
    let profile = db
        .create_profile(
            body.name.trim(),
            body.provider_name.as_deref().unwrap_or("bedrock"),
            body.enabled.unwrap_or(true),
            body.guardrail_id.trim(),
            body.guardrail_version.as_deref().unwrap_or("1"),
            body.region.as_deref().unwrap_or("us-east-1"),
            body.access_key.trim(),
            body.secret_key.trim(),
        )
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(profile_id = %profile.id, name = %profile.name, "guardrail_profile_created");

    reload_engine(&state).await;
    Ok(Json(profile_to_json(&profile)))
}

/// GET /guardrails/profiles/:id — Get profile
async fn get_profile(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let db = require_guardrails_db(&state)?;
    let profile = db
        .get_profile(id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("Profile '{}' not found", id)))?;

    Ok(Json(profile_to_json(&profile)))
}

/// PUT /guardrails/profiles/:id — Update profile
async fn update_profile(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "Profile name cannot be empty".to_string(),
        ));
    }

    let db = require_guardrails_db(&state)?;
    let updated = db
        .update_profile(
            id,
            body.name.trim(),
            body.provider_name.as_deref().unwrap_or("bedrock"),
            body.enabled.unwrap_or(true),
            body.guardrail_id.trim(),
            body.guardrail_version.as_deref().unwrap_or("1"),
            body.region.as_deref().unwrap_or("us-east-1"),
            body.access_key.trim(),
            body.secret_key.as_deref(),
        )
        .await
        .map_err(ApiError::Internal)?;

    if !updated {
        return Err(ApiError::NotFound(format!(
            "Profile '{}' not found",
            id
        )));
    }

    tracing::info!(profile_id = %id, "guardrail_profile_updated");

    reload_engine(&state).await;

    // Re-fetch to return updated data
    let profile = db
        .get_profile(id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("Profile '{}' not found", id)))?;

    Ok(Json(profile_to_json(&profile)))
}

/// DELETE /guardrails/profiles/:id — Delete profile
async fn delete_profile(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let db = require_guardrails_db(&state)?;
    let deleted = db.delete_profile(id).await.map_err(ApiError::Internal)?;

    if !deleted {
        return Err(ApiError::NotFound(format!(
            "Profile '{}' not found",
            id
        )));
    }

    tracing::info!(profile_id = %id, "guardrail_profile_deleted");

    reload_engine(&state).await;
    Ok(Json(json!({ "ok": true })))
}

// ── Rule handlers ────────────────────────────────────────────────────

/// GET /guardrails/rules — List all rules (with profile_ids)
async fn list_rules(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
) -> Result<Json<Value>, ApiError> {
    let db = require_guardrails_db(&state)?;
    let rules = db.list_rules().await.map_err(ApiError::Internal)?;

    let items: Vec<Value> = rules.iter().map(rule_to_json).collect();
    Ok(Json(json!({ "rules": items, "count": items.len() })))
}

/// POST /guardrails/rules — Create rule
async fn create_rule(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Json(body): Json<CreateRuleRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "Rule name cannot be empty".to_string(),
        ));
    }

    // Validate CEL expression if provided
    let cel_expr = body.cel_expression.as_deref().unwrap_or("");
    if !cel_expr.is_empty() {
        crate::guardrails::cel::CelEvaluator::validate(cel_expr).map_err(|e| {
            ApiError::ValidationError(format!("Invalid CEL expression: {}", e))
        })?;
    }

    let apply_to = body
        .apply_to
        .as_deref()
        .map(ApplyTo::parse_str)
        .unwrap_or(ApplyTo::Both);

    let sampling_rate = body.sampling_rate.unwrap_or(100).clamp(0, 100);

    let db = require_guardrails_db(&state)?;
    let rule = db
        .create_rule(
            body.name.trim(),
            body.description.as_deref().unwrap_or(""),
            body.enabled.unwrap_or(true),
            cel_expr,
            &apply_to,
            sampling_rate,
            body.timeout_ms.unwrap_or(5000),
            body.profile_ids.as_deref().unwrap_or(&[]),
        )
        .await
        .map_err(ApiError::Internal)?;

    tracing::info!(rule_id = %rule.id, name = %rule.name, "guardrail_rule_created");

    reload_engine(&state).await;
    Ok(Json(rule_to_json(&rule)))
}

/// GET /guardrails/rules/:id — Get rule
async fn get_rule(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let db = require_guardrails_db(&state)?;
    let rule = db
        .get_rule(id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("Rule '{}' not found", id)))?;

    Ok(Json(rule_to_json(&rule)))
}

/// PUT /guardrails/rules/:id — Update rule
async fn update_rule(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRuleRequest>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "Rule name cannot be empty".to_string(),
        ));
    }

    let cel_expr = body.cel_expression.as_deref().unwrap_or("");
    if !cel_expr.is_empty() {
        crate::guardrails::cel::CelEvaluator::validate(cel_expr).map_err(|e| {
            ApiError::ValidationError(format!("Invalid CEL expression: {}", e))
        })?;
    }

    let apply_to = body
        .apply_to
        .as_deref()
        .map(ApplyTo::parse_str)
        .unwrap_or(ApplyTo::Both);

    let sampling_rate = body.sampling_rate.unwrap_or(100).clamp(0, 100);

    let db = require_guardrails_db(&state)?;
    let updated = db
        .update_rule(
            id,
            body.name.trim(),
            body.description.as_deref().unwrap_or(""),
            body.enabled.unwrap_or(true),
            cel_expr,
            &apply_to,
            sampling_rate,
            body.timeout_ms.unwrap_or(5000),
            body.profile_ids.as_deref().unwrap_or(&[]),
        )
        .await
        .map_err(ApiError::Internal)?;

    if !updated {
        return Err(ApiError::NotFound(format!(
            "Rule '{}' not found",
            id
        )));
    }

    tracing::info!(rule_id = %id, "guardrail_rule_updated");

    reload_engine(&state).await;

    let rule = db
        .get_rule(id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("Rule '{}' not found", id)))?;

    Ok(Json(rule_to_json(&rule)))
}

/// DELETE /guardrails/rules/:id — Delete rule
async fn delete_rule(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let db = require_guardrails_db(&state)?;
    let deleted = db.delete_rule(id).await.map_err(ApiError::Internal)?;

    if !deleted {
        return Err(ApiError::NotFound(format!(
            "Rule '{}' not found",
            id
        )));
    }

    tracing::info!(rule_id = %id, "guardrail_rule_deleted");

    reload_engine(&state).await;
    Ok(Json(json!({ "ok": true })))
}

// ── Test & validation handlers ───────────────────────────────────────

/// POST /guardrails/test — Test a profile by calling Bedrock with sample content
async fn test_profile(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionInfo>,
    Json(body): Json<TestProfileRequest>,
) -> Result<Json<TestProfileResponse>, ApiError> {
    if body.content.trim().is_empty() {
        return Err(ApiError::ValidationError(
            "Test content cannot be empty".to_string(),
        ));
    }

    let db = require_guardrails_db(&state)?;
    let profile = db
        .get_profile(body.profile_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::NotFound(format!("Profile '{}' not found", body.profile_id))
        })?;

    // Reuse the engine's Bedrock client if available, otherwise create a temporary one
    let fallback_client;
    let client: &super::bedrock::BedrockGuardrailClient =
        if let Some(ref engine) = state.guardrails_engine {
            engine.bedrock_client()
        } else {
            fallback_client =
                super::bedrock::BedrockGuardrailClient::new(std::time::Duration::from_secs(10));
            &fallback_client
        };

    let start = std::time::Instant::now();
    match client
        .apply_guardrail(&profile, &body.content, "INPUT")
        .await
    {
        Ok(response) => {
            let response_time_ms = start.elapsed().as_millis() as u64;
            let action_str = match response.action {
                crate::guardrails::types::GuardrailAction::None => "NONE",
                crate::guardrails::types::GuardrailAction::Intervened => "GUARDRAIL_INTERVENED",
                crate::guardrails::types::GuardrailAction::Redacted => "REDACTED",
            };
            Ok(Json(TestProfileResponse {
                success: true,
                action: action_str.to_string(),
                response_time_ms,
                error: None,
            }))
        }
        Err(e) => {
            let response_time_ms = start.elapsed().as_millis() as u64;
            Ok(Json(TestProfileResponse {
                success: false,
                action: "ERROR".to_string(),
                response_time_ms,
                error: Some(e.to_string()),
            }))
        }
    }
}

/// POST /guardrails/cel/validate — Validate CEL expression syntax
async fn validate_cel(
    Json(body): Json<ValidateCelRequest>,
) -> Result<Json<ValidateCelResponse>, ApiError> {
    match crate::guardrails::cel::CelEvaluator::validate(&body.expression) {
        Ok(()) => Ok(Json(ValidateCelResponse {
            valid: true,
            error: None,
        })),
        Err(e) => Ok(Json(ValidateCelResponse {
            valid: false,
            error: Some(e.to_string()),
        })),
    }
}

// ── Router ───────────────────────────────────────────────────────────

/// Build the guardrails admin API router.
///
/// All routes are admin-only — the middleware stack (session + CSRF + admin)
/// is applied by `web_ui_routes()` in `web_ui/mod.rs`.
pub fn guardrails_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/guardrails/profiles",
            get(list_profiles).post(create_profile),
        )
        .route(
            "/guardrails/profiles/{id}",
            get(get_profile)
                .put(update_profile)
                .delete(delete_profile),
        )
        .route("/guardrails/rules", get(list_rules).post(create_rule))
        .route(
            "/guardrails/rules/{id}",
            get(get_rule).put(update_rule).delete(delete_rule),
        )
        .route("/guardrails/test", post(test_profile))
        .route("/guardrails/cel/validate", post(validate_cel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret_long() {
        assert_eq!(mask_secret("abcdefghij"), "abcd...ghij");
    }

    #[test]
    fn test_mask_secret_short() {
        assert_eq!(mask_secret("short"), "****");
    }

    #[test]
    fn test_mask_secret_empty() {
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn test_mask_secret_boundary() {
        // Exactly 8 chars — too short to show prefix/suffix
        assert_eq!(mask_secret("12345678"), "****");
        // 9 chars — just enough
        assert_eq!(mask_secret("123456789"), "1234...6789");
    }

    #[test]
    fn test_profile_to_json_masks_secret() {
        let profile = GuardrailProfile {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            provider_name: "bedrock".to_string(),
            enabled: true,
            guardrail_id: "guard-123".to_string(),
            guardrail_version: "1".to_string(),
            region: "us-east-1".to_string(),
            access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let json = profile_to_json(&profile);
        let secret = json["secret_key"].as_str().unwrap();
        assert!(secret.contains("..."));
        assert!(!secret.contains("EXAMPLEKEY"));
    }

    #[test]
    fn test_rule_to_json_includes_profile_ids() {
        let pid1 = Uuid::new_v4();
        let pid2 = Uuid::new_v4();
        let rule = GuardrailRule {
            id: Uuid::new_v4(),
            name: "test rule".to_string(),
            description: "desc".to_string(),
            enabled: true,
            cel_expression: "".to_string(),
            apply_to: ApplyTo::Both,
            sampling_rate: 100,
            timeout_ms: 5000,
            profile_ids: vec![pid1, pid2],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let json = rule_to_json(&rule);
        let ids = json["profile_ids"].as_array().unwrap();
        assert_eq!(ids.len(), 2);
    }
}
