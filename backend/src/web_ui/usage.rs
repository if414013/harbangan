use axum::extract::{Extension, Query, State};
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::routes::{AppState, SessionInfo};
use crate::web_ui::config_db::{UsageSummary, UserUsageSummary};

/// Query parameters for usage endpoints
#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    /// Start date in YYYY-MM-DD format (default: 30 days ago)
    #[serde(default)]
    pub start_date: Option<String>,
    /// End date in YYYY-MM-DD format (default: today)
    #[serde(default)]
    pub end_date: Option<String>,
    /// Group by: "day", "model", or "provider" (default: "day")
    #[serde(default = "default_group_by")]
    pub group_by: String,
}

fn default_group_by() -> String {
    "day".to_string()
}

/// Usage summary response
#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub start_date: String,
    pub end_date: String,
    pub group_by: String,
    pub data: Vec<UsageSummary>,
}

/// User usage summary response (admin only)
#[derive(Debug, Serialize)]
pub struct UserUsageResponse {
    pub start_date: String,
    pub end_date: String,
    pub data: Vec<UserUsageSummary>,
}

/// GET /_ui/api/usage - Get usage summary for the current user
pub async fn get_usage(
    State(state): State<AppState>,
    Extension(session): Extension<SessionInfo>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, ApiError> {
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not available")))?;

    // Set default date range if not provided
    let end_date = query
        .end_date
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    let start_date = query.start_date.unwrap_or_else(|| {
        (Utc::now() - Duration::days(30))
            .format("%Y-%m-%d")
            .to_string()
    });

    // Validate group_by
    let group_by = match query.group_by.as_str() {
        "day" | "model" | "provider" => query.group_by,
        _ => "day".to_string(),
    };

    let data = config_db
        .get_usage_summary(Some(session.user_id), &start_date, &end_date, &group_by)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(UsageResponse {
        start_date,
        end_date,
        group_by,
        data,
    }))
}

/// GET /_ui/api/admin/usage - Get global usage summary (admin only)
pub async fn get_admin_usage(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, ApiError> {
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not available")))?;

    // Set default date range if not provided
    let end_date = query
        .end_date
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    let start_date = query.start_date.unwrap_or_else(|| {
        (Utc::now() - Duration::days(30))
            .format("%Y-%m-%d")
            .to_string()
    });

    // Validate group_by
    let group_by = match query.group_by.as_str() {
        "day" | "model" | "provider" => query.group_by,
        _ => "day".to_string(),
    };

    let data = config_db
        .get_usage_summary(None, &start_date, &end_date, &group_by)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(UsageResponse {
        start_date,
        end_date,
        group_by,
        data,
    }))
}

/// GET /_ui/api/admin/usage/users - Get usage summary grouped by user (admin only)
pub async fn get_admin_usage_by_users(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UserUsageResponse>, ApiError> {
    let config_db = state
        .config_db
        .as_ref()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Database not available")))?;

    // Set default date range if not provided
    let end_date = query
        .end_date
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    let start_date = query.start_date.unwrap_or_else(|| {
        (Utc::now() - Duration::days(30))
            .format("%Y-%m-%d")
            .to_string()
    });

    let data = config_db
        .get_usage_by_users(&start_date, &end_date)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(UserUsageResponse {
        start_date,
        end_date,
        data,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_group_by() {
        assert_eq!(default_group_by(), "day");
    }

    #[test]
    fn test_usage_query_defaults() {
        // Test that query struct can be created with defaults
        let query: UsageQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(query.group_by, "day");
        assert!(query.start_date.is_none());
        assert!(query.end_date.is_none());
    }

    #[test]
    fn test_usage_query_custom_group_by() {
        let query: UsageQuery = serde_json::from_str(r#"{"group_by": "model"}"#).unwrap();
        assert_eq!(query.group_by, "model");
    }
}
