use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// When to apply a guardrail rule: input, output, or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplyTo {
    Input,
    Output,
    Both,
}

impl ApplyTo {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApplyTo::Input => "input",
            ApplyTo::Output => "output",
            ApplyTo::Both => "both",
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "input" => ApplyTo::Input,
            "output" => ApplyTo::Output,
            _ => ApplyTo::Both,
        }
    }
}

/// Result of a guardrail evaluation: pass, block, or redact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GuardrailAction {
    None,
    Intervened,
    Redacted,
}

/// A single violation detected by a guardrail profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailViolation {
    pub violation_type: String,
    pub category: String,
    pub severity: String,
    pub action: GuardrailAction,
    pub message: String,
}

/// Per-rule, per-profile validation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailValidationResult {
    pub rule_id: Uuid,
    pub profile_id: Uuid,
    pub action: GuardrailAction,
    pub violations: Vec<GuardrailViolation>,
    pub processing_time_ms: u64,
}

/// Aggregate result across all rules/profiles for a single check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailCheckResult {
    pub passed: bool,
    pub action: GuardrailAction,
    pub results: Vec<GuardrailValidationResult>,
    pub total_processing_time_ms: u64,
}

/// Context about the current request, exposed to CEL expressions.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub model: String,
    pub api_format: String,
    pub message_count: usize,
    pub has_tools: bool,
    pub is_streaming: bool,
    pub content_length: usize,
}

/// A guardrail rule: WHEN to validate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailRule {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub cel_expression: String,
    pub apply_to: ApplyTo,
    pub sampling_rate: i16,
    pub timeout_ms: i32,
    pub profile_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A guardrail profile: HOW to validate (Bedrock config).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailProfile {
    pub id: Uuid,
    pub name: String,
    pub provider_name: String,
    pub enabled: bool,
    pub guardrail_id: String,
    pub guardrail_version: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// In-memory snapshot of guardrails configuration.
#[derive(Debug, Clone)]
pub struct GuardrailsConfig {
    pub enabled: bool,
    pub rules: Vec<GuardrailRule>,
    pub profiles: Vec<GuardrailProfile>,
}
