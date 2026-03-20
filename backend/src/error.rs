use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// API errors that can occur during request processing
#[derive(Error, Debug)]
pub enum ApiError {
    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthError(String),

    /// Invalid model name
    #[error("Invalid model: {0}")]
    #[allow(dead_code)]
    InvalidModel(String),

    /// Error from Kiro API
    #[error("Kiro API error: {status} - {message}")]
    #[allow(clippy::enum_variant_names)]
    KiroApiError { status: u16, message: String },

    /// Configuration error
    #[error("Configuration error: {0}")]
    #[allow(dead_code)]
    ConfigError(String),

    /// Request validation error
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Forbidden (authenticated but not authorized)
    #[error("Forbidden: {0}")]
    #[allow(dead_code)]
    Forbidden(String),

    /// Session expired or invalid
    #[error("Session expired")]
    #[allow(dead_code)]
    SessionExpired,

    /// Email domain not in allowlist
    #[error("Domain not allowed: {0}")]
    #[allow(dead_code)]
    DomainNotAllowed(String),

    /// User has no Kiro token configured
    #[error("Kiro token required")]
    #[allow(dead_code)]
    KiroTokenRequired,

    /// User's Kiro token has expired and refresh failed
    #[error("Kiro token expired")]
    #[allow(dead_code)]
    KiroTokenExpired,

    /// Cannot remove or demote the last admin
    #[error("Cannot remove or demote the last admin user")]
    #[allow(dead_code)]
    LastAdmin,

    /// Content blocked by guardrail policy
    #[error("Content blocked by guardrail policy")]
    #[allow(dead_code)]
    GuardrailBlocked {
        violations: Vec<crate::guardrails::GuardrailValidationResult>,
        processing_time_ms: u64,
    },

    /// Content redacted by guardrail (warning)
    #[error("Content redacted by guardrail")]
    #[allow(dead_code)]
    GuardrailWarning {
        violations: Vec<crate::guardrails::GuardrailValidationResult>,
        processing_time_ms: u64,
        redacted_content: String,
    },

    /// Generic not found
    #[error("Not found: {0}")]
    #[allow(dead_code)]
    NotFound(String),

    /// Error returned by a direct provider API (Anthropic, OpenAI, etc.)
    #[error("Provider API error ({provider}): {status} - {message}")]
    #[allow(dead_code, clippy::enum_variant_names)]
    ProviderApiError {
        provider: String,
        status: u16,
        message: String,
        /// Response headers from the provider (for extracting Retry-After on 429).
        #[allow(dead_code)]
        headers: Option<axum::http::HeaderMap>,
    },

    /// User has not configured a key for the required provider
    #[error("Provider not configured: {0}")]
    #[allow(dead_code)]
    ProviderNotConfigured(String),

    /// Invalid credentials (wrong email/password)
    #[error("Invalid credentials")]
    #[allow(dead_code)]
    InvalidCredentials,

    /// Account temporarily locked due to too many failed login attempts
    #[error("Account temporarily locked")]
    #[allow(dead_code)]
    AccountLocked { retry_after_secs: u64 },

    /// Two-factor authentication required to complete login
    #[error("Two-factor authentication required")]
    #[allow(dead_code)]
    TwoFactorRequired { login_token: String },

    /// Two-factor setup required before account can be used
    #[error("Two-factor setup required")]
    #[allow(dead_code)]
    TwoFactorSetupRequired,

    /// Copilot authentication failed (GitHub OAuth or token exchange)
    #[error("Copilot auth failed: {0}")]
    #[allow(dead_code)]
    CopilotAuthError(String),

    /// Copilot bearer token has expired and needs re-authentication
    #[error("Copilot token expired")]
    #[allow(dead_code)]
    CopilotTokenExpired,

    /// Rate limited — too many requests for a provider credential
    #[error("Rate limited: retry after {retry_after_secs}s")]
    #[allow(dead_code)]
    RateLimited {
        provider: String,
        retry_after_secs: u64,
    },

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            ApiError::AuthError(msg) => (StatusCode::UNAUTHORIZED, "auth_error", msg),
            ApiError::InvalidModel(msg) => (StatusCode::BAD_REQUEST, "invalid_model", msg),
            ApiError::KiroApiError { status, message } => {
                let status_code =
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                (status_code, "kiro_api_error", message)
            }
            ApiError::ConfigError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "config_error", msg),
            ApiError::ValidationError(msg) => (StatusCode::BAD_REQUEST, "validation_error", msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg),
            ApiError::SessionExpired => (
                StatusCode::UNAUTHORIZED,
                "session_expired",
                "Session expired".to_string(),
            ),
            ApiError::DomainNotAllowed(domain) => (
                StatusCode::FORBIDDEN,
                "domain_not_allowed",
                format!("Email domain '{}' is not in the allowlist", domain),
            ),
            ApiError::KiroTokenRequired => (
                StatusCode::FORBIDDEN,
                "kiro_token_required",
                "Set up your Kiro token at /_ui/profile".to_string(),
            ),
            ApiError::KiroTokenExpired => (
                StatusCode::FORBIDDEN,
                "kiro_token_expired",
                "Re-authenticate your Kiro token at /_ui/profile".to_string(),
            ),
            ApiError::LastAdmin => (
                StatusCode::CONFLICT,
                "last_admin",
                "Cannot remove or demote the last admin user".to_string(),
            ),
            ApiError::GuardrailBlocked {
                ref violations,
                processing_time_ms,
            } => {
                let body = Json(json!({
                    "error": {
                        "message": "Content blocked by guardrail policy",
                        "type": "guardrail_blocked",
                        "violations": violations,
                        "processing_time_ms": processing_time_ms,
                    }
                }));
                return (StatusCode::FORBIDDEN, body).into_response();
            }
            ApiError::GuardrailWarning {
                ref violations,
                processing_time_ms,
                ref redacted_content,
            } => {
                let body = Json(json!({
                    "error": {
                        "message": "Content redacted by guardrail",
                        "type": "guardrail_warning",
                        "violations": violations,
                        "processing_time_ms": processing_time_ms,
                        "redacted_content": redacted_content,
                    }
                }));
                return (StatusCode::OK, body).into_response();
            }
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg),
            ApiError::ProviderApiError {
                provider,
                status,
                message,
                ..
            } => {
                let status_code =
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                (
                    status_code,
                    "provider_api_error",
                    format!("[{}] {}", provider, message),
                )
            }
            ApiError::ProviderNotConfigured(provider) => (
                StatusCode::FORBIDDEN,
                "provider_not_configured",
                format!(
                    "No API key configured for provider '{}'. Connect it at /_ui/profile",
                    provider
                ),
            ),
            ApiError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "Invalid credentials".to_string(),
            ),
            ApiError::AccountLocked { retry_after_secs } => {
                let body = Json(json!({
                    "error": {
                        "message": format!("Account temporarily locked. Retry after {}s.", retry_after_secs),
                        "type": "account_locked",
                        "retry_after": retry_after_secs,
                    }
                }));
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("retry-after", retry_after_secs.to_string())],
                    body,
                )
                    .into_response();
            }
            ApiError::TwoFactorRequired { ref login_token } => {
                let body = Json(json!({
                    "needs_2fa": true,
                    "login_token": login_token,
                }));
                return (StatusCode::OK, body).into_response();
            }
            ApiError::TwoFactorSetupRequired => (
                StatusCode::FORBIDDEN,
                "totp_setup_required",
                "Two-factor setup required".to_string(),
            ),
            ApiError::CopilotAuthError(msg) => (StatusCode::BAD_GATEWAY, "copilot_auth_error", msg),
            ApiError::CopilotTokenExpired => (
                StatusCode::FORBIDDEN,
                "copilot_token_expired",
                "Copilot token expired. Re-connect at /_ui/profile".to_string(),
            ),
            ApiError::RateLimited {
                ref provider,
                retry_after_secs,
            } => {
                let body = Json(json!({
                    "error": {
                        "message": format!("Rate limited for provider '{}'. Retry after {}s.", provider, retry_after_secs),
                        "type": "rate_limited",
                        "retry_after": retry_after_secs,
                    }
                }));
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("retry-after", retry_after_secs.to_string())],
                    body,
                )
                    .into_response();
            }
            ApiError::Internal(err) => {
                // Log internal errors
                tracing::error!("Internal error: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "Internal server error".to_string(),
                )
            }
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "type": error_type,
            }
        }));

        (status, body).into_response()
    }
}

/// Result type alias for API operations
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
        let err = ApiError::AuthError("Invalid token".to_string());
        assert_eq!(err.to_string(), "Authentication failed: Invalid token");

        let err = ApiError::InvalidModel("gpt-4".to_string());
        assert_eq!(err.to_string(), "Invalid model: gpt-4");

        let err = ApiError::KiroApiError {
            status: 429,
            message: "Rate limit exceeded".to_string(),
        };
        assert_eq!(err.to_string(), "Kiro API error: 429 - Rate limit exceeded");
    }

    #[test]
    fn test_config_error_message() {
        let err = ApiError::ConfigError("Missing API key".to_string());
        assert_eq!(err.to_string(), "Configuration error: Missing API key");
    }

    #[test]
    fn test_validation_error_message() {
        let err = ApiError::ValidationError("Invalid JSON".to_string());
        assert_eq!(err.to_string(), "Validation error: Invalid JSON");
    }

    #[test]
    fn test_internal_error_message() {
        let err = ApiError::Internal(anyhow::anyhow!("Something went wrong"));
        assert_eq!(err.to_string(), "Internal error: Something went wrong");
    }

    #[tokio::test]
    async fn test_error_response_conversion() {
        let err = ApiError::AuthError("Invalid token".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let err = ApiError::InvalidModel("gpt-4".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let err = ApiError::KiroApiError {
            status: 429,
            message: "Rate limit".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_config_error_response() {
        let err = ApiError::ConfigError("Bad config".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_validation_error_response() {
        let err = ApiError::ValidationError("Missing field".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_internal_error_response() {
        let err = ApiError::Internal(anyhow::anyhow!("Unexpected error"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_kiro_api_error_invalid_status() {
        // Test with an invalid status code (must be >= 1000 to be invalid)
        // HTTP status codes 100-999 are valid
        let err = ApiError::KiroApiError {
            status: 1000, // Invalid HTTP status (out of range)
            message: "Unknown error".to_string(),
        };
        let response = err.into_response();
        // Invalid status codes fall back to 500 Internal Server Error
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_kiro_api_error_various_statuses() {
        // Test 400 Bad Request
        let err = ApiError::KiroApiError {
            status: 400,
            message: "Bad request".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Test 403 Forbidden
        let err = ApiError::KiroApiError {
            status: 403,
            message: "Forbidden".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // Test 404 Not Found
        let err = ApiError::KiroApiError {
            status: 404,
            message: "Not found".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 500 Internal Server Error
        let err = ApiError::KiroApiError {
            status: 500,
            message: "Server error".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Test 503 Service Unavailable
        let err = ApiError::KiroApiError {
            status: 503,
            message: "Service unavailable".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_forbidden_returns_403() {
        let err = ApiError::Forbidden("Access denied".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_session_expired_returns_401() {
        let err = ApiError::SessionExpired;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_domain_not_allowed_returns_403() {
        let err = ApiError::DomainNotAllowed("evil.com".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_kiro_token_required_returns_403() {
        let err = ApiError::KiroTokenRequired;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_kiro_token_expired_returns_403() {
        let err = ApiError::KiroTokenExpired;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_last_admin_returns_409() {
        let err = ApiError::LastAdmin;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_rate_limited_returns_429_with_retry_after() {
        let err = ApiError::RateLimited {
            provider: "qwen".to_string(),
            retry_after_secs: 42,
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "42");
    }
}
