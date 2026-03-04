use std::time::Duration;

use anyhow::{Context, Result};
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    sign, SignableBody, SignableRequest, SignatureLocation, SigningParams, SigningSettings,
};
use aws_sigv4::sign::v4;
use serde_json::{json, Value};

use super::types::{GuardrailAction, GuardrailProfile, GuardrailViolation};

/// Validate that a region string matches expected AWS region format (e.g. "us-east-1").
fn validate_region(region: &str) -> Result<()> {
    // Match: us-east-1, eu-west-2, ap-southeast-1, etc.
    let is_valid = region.len() <= 25
        && region
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');

    if !is_valid || region.is_empty() {
        anyhow::bail!("Invalid AWS region format: '{}'", region);
    }

    // Must match pattern: xx-xxxx-N (e.g. us-east-1)
    let parts: Vec<&str> = region.split('-').collect();
    if parts.len() < 3 {
        anyhow::bail!(
            "Invalid AWS region format: '{}' (expected e.g. us-east-1)",
            region
        );
    }

    // Last part must end with a digit
    if !parts
        .last()
        .is_some_and(|p| p.chars().last().is_some_and(|c| c.is_ascii_digit()))
    {
        anyhow::bail!(
            "Invalid AWS region format: '{}' (expected trailing number)",
            region
        );
    }

    Ok(())
}

/// Response from the Bedrock ApplyGuardrail API.
#[derive(Debug, Clone)]
pub struct BedrockGuardrailResponse {
    pub action: GuardrailAction,
    pub violations: Vec<GuardrailViolation>,
    pub output_text: Option<String>,
}

/// Client for calling the AWS Bedrock ApplyGuardrail API with SigV4 signing.
pub struct BedrockGuardrailClient {
    http: reqwest::Client,
}

impl BedrockGuardrailClient {
    pub fn new(timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to build HTTP client for Bedrock");
        Self { http }
    }

    /// Call the Bedrock ApplyGuardrail API for a given profile and content.
    ///
    /// `source` must be `"INPUT"` or `"OUTPUT"`.
    pub async fn apply_guardrail(
        &self,
        profile: &GuardrailProfile,
        content: &str,
        source: &str,
    ) -> Result<BedrockGuardrailResponse> {
        // Validate region to prevent SSRF via crafted region strings
        validate_region(&profile.region)?;

        let qualifier = if source == "INPUT" {
            "query"
        } else {
            "guard_content"
        };

        let body = json!({
            "source": source,
            "content": [{
                "text": {
                    "text": content,
                    "qualifiers": [qualifier]
                }
            }]
        });

        let body_bytes = serde_json::to_vec(&body)?;

        let url = format!(
            "https://bedrock-runtime.{}.amazonaws.com/guardrail/{}/version/{}/apply",
            profile.region, profile.guardrail_id, profile.guardrail_version
        );

        // Build SigV4 signing params
        let credentials = Credentials::new(
            &profile.access_key,
            &profile.secret_key,
            None,
            None,
            "guardrails",
        );
        let identity = credentials.into();
        let mut settings = SigningSettings::default();
        settings.signature_location = SignatureLocation::Headers;

        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&profile.region)
            .name("bedrock")
            .time(std::time::SystemTime::now())
            .settings(settings)
            .build()
            .context("Failed to build SigV4 signing params")?;

        let signable_request = SignableRequest::new(
            "POST",
            &url,
            std::iter::once(("content-type", "application/json")),
            SignableBody::Bytes(&body_bytes),
        )
        .context("Failed to create signable request")?;

        let (signing_instructions, _signature) =
            sign(signable_request, &SigningParams::V4(signing_params))
                .context("Failed to sign request")?
                .into_parts();

        // Build reqwest request with signed headers
        let mut req = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .body(body_bytes);

        // Apply signing headers from the instructions
        let (headers, _params) = signing_instructions.into_parts();
        for header in &headers {
            req = req.header(header.name(), header.value());
        }

        let response = req.send().await.context("Failed to call Bedrock API")?;

        let status = response.status();
        let response_body: Value = response
            .json()
            .await
            .context("Failed to parse Bedrock response")?;

        if !status.is_success() {
            let msg = response_body["message"]
                .as_str()
                .unwrap_or("Unknown Bedrock error");
            anyhow::bail!("Bedrock API error ({}): {}", status.as_u16(), msg);
        }

        Self::parse_response(&response_body)
    }

    /// Parse the Bedrock ApplyGuardrail API response.
    fn parse_response(body: &Value) -> Result<BedrockGuardrailResponse> {
        let action_str = body["action"].as_str().unwrap_or("NONE");
        let action = match action_str {
            "GUARDRAIL_INTERVENED" => GuardrailAction::Intervened,
            _ => GuardrailAction::None,
        };

        let mut violations = Vec::new();
        if let Some(assessments) = body["assessments"].as_array() {
            for assessment in assessments {
                // Content policy violations
                if let Some(policy) = assessment.get("contentPolicy") {
                    if let Some(filters) = policy["filters"].as_array() {
                        for filter in filters {
                            let filter_action = match filter["action"].as_str().unwrap_or("NONE") {
                                "BLOCKED" => GuardrailAction::Intervened,
                                _ => GuardrailAction::None,
                            };
                            violations.push(GuardrailViolation {
                                violation_type: "content_policy".to_string(),
                                category: filter["type"].as_str().unwrap_or("unknown").to_string(),
                                severity: filter["confidence"]
                                    .as_str()
                                    .unwrap_or("NONE")
                                    .to_string(),
                                action: filter_action,
                                message: format!(
                                    "Content policy violation: {}",
                                    filter["type"].as_str().unwrap_or("unknown")
                                ),
                            });
                        }
                    }
                }

                // Topic policy violations
                if let Some(policy) = assessment.get("topicPolicy") {
                    if let Some(topics) = policy["topics"].as_array() {
                        for topic in topics {
                            let topic_action = match topic["action"].as_str().unwrap_or("NONE") {
                                "BLOCKED" => GuardrailAction::Intervened,
                                _ => GuardrailAction::None,
                            };
                            violations.push(GuardrailViolation {
                                violation_type: "topic_policy".to_string(),
                                category: topic["name"].as_str().unwrap_or("unknown").to_string(),
                                severity: "HIGH".to_string(),
                                action: topic_action,
                                message: format!(
                                    "Topic policy violation: {}",
                                    topic["name"].as_str().unwrap_or("unknown")
                                ),
                            });
                        }
                    }
                }

                // Word policy violations
                if let Some(policy) = assessment.get("wordPolicy") {
                    if let Some(words) = policy["customWords"].as_array() {
                        for word in words {
                            violations.push(GuardrailViolation {
                                violation_type: "word_policy".to_string(),
                                category: "custom_word".to_string(),
                                severity: "HIGH".to_string(),
                                action: GuardrailAction::Intervened,
                                message: format!(
                                    "Word policy violation: {}",
                                    word["match"].as_str().unwrap_or("redacted")
                                ),
                            });
                        }
                    }
                }

                // Sensitive information policy violations
                if let Some(policy) = assessment.get("sensitiveInformationPolicy") {
                    if let Some(pii) = policy["piiEntities"].as_array() {
                        for entity in pii {
                            let pii_action = match entity["action"].as_str().unwrap_or("NONE") {
                                "BLOCKED" => GuardrailAction::Intervened,
                                "ANONYMIZED" => GuardrailAction::Redacted,
                                _ => GuardrailAction::None,
                            };
                            violations.push(GuardrailViolation {
                                violation_type: "sensitive_info".to_string(),
                                category: entity["type"].as_str().unwrap_or("unknown").to_string(),
                                severity: "MEDIUM".to_string(),
                                action: pii_action,
                                message: format!(
                                    "Sensitive information detected: {}",
                                    entity["type"].as_str().unwrap_or("unknown")
                                ),
                            });
                        }
                    }
                }
            }
        }

        // Extract output text if present (for redacted content)
        let output_text = body["outputs"]
            .as_array()
            .and_then(|outputs| outputs.first())
            .and_then(|o| o["text"].as_str())
            .map(|s| s.to_string());

        Ok(BedrockGuardrailResponse {
            action,
            violations,
            output_text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_region_valid() {
        assert!(validate_region("us-east-1").is_ok());
        assert!(validate_region("eu-west-2").is_ok());
        assert!(validate_region("ap-southeast-1").is_ok());
    }

    #[test]
    fn test_validate_region_invalid() {
        assert!(validate_region("").is_err());
        assert!(validate_region("evil.example.com").is_err());
        assert!(validate_region("us-east-1.evil.com/x").is_err());
        assert!(validate_region("UPPERCASE").is_err());
        assert!(validate_region("us").is_err());
    }

    #[test]
    fn test_parse_response_none() {
        let body = json!({
            "action": "NONE",
            "outputs": [],
            "assessments": []
        });
        let result = BedrockGuardrailClient::parse_response(&body).unwrap();
        assert_eq!(result.action, GuardrailAction::None);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_parse_response_intervened() {
        let body = json!({
            "action": "GUARDRAIL_INTERVENED",
            "outputs": [{"text": "I cannot help with that."}],
            "assessments": [{
                "contentPolicy": {
                    "filters": [{
                        "type": "HATE",
                        "confidence": "HIGH",
                        "action": "BLOCKED"
                    }]
                }
            }]
        });
        let result = BedrockGuardrailClient::parse_response(&body).unwrap();
        assert_eq!(result.action, GuardrailAction::Intervened);
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].violation_type, "content_policy");
        assert_eq!(result.violations[0].category, "HATE");
        assert_eq!(
            result.output_text,
            Some("I cannot help with that.".to_string())
        );
    }

    #[test]
    fn test_parse_response_topic_policy() {
        let body = json!({
            "action": "GUARDRAIL_INTERVENED",
            "outputs": [],
            "assessments": [{
                "topicPolicy": {
                    "topics": [{
                        "name": "Financial Advice",
                        "type": "DENY",
                        "action": "BLOCKED"
                    }]
                }
            }]
        });
        let result = BedrockGuardrailClient::parse_response(&body).unwrap();
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].violation_type, "topic_policy");
        assert_eq!(result.violations[0].category, "Financial Advice");
    }

    #[test]
    fn test_parse_response_sensitive_info() {
        let body = json!({
            "action": "GUARDRAIL_INTERVENED",
            "outputs": [{"text": "Content with [REDACTED] info."}],
            "assessments": [{
                "sensitiveInformationPolicy": {
                    "piiEntities": [{
                        "type": "EMAIL",
                        "action": "ANONYMIZED",
                        "match": "user@example.com"
                    }]
                }
            }]
        });
        let result = BedrockGuardrailClient::parse_response(&body).unwrap();
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].violation_type, "sensitive_info");
        assert_eq!(result.violations[0].action, GuardrailAction::Redacted);
    }

    #[test]
    fn test_parse_response_multiple_assessments() {
        let body = json!({
            "action": "GUARDRAIL_INTERVENED",
            "outputs": [],
            "assessments": [{
                "contentPolicy": {
                    "filters": [
                        {"type": "HATE", "confidence": "HIGH", "action": "BLOCKED"},
                        {"type": "VIOLENCE", "confidence": "MEDIUM", "action": "BLOCKED"}
                    ]
                },
                "wordPolicy": {
                    "customWords": [{"match": "badword", "action": "BLOCKED"}]
                }
            }]
        });
        let result = BedrockGuardrailClient::parse_response(&body).unwrap();
        assert_eq!(result.violations.len(), 3);
    }
}
