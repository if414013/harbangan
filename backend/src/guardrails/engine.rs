use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::RwLock;

use super::bedrock::BedrockGuardrailClient;
use super::cel::CelEvaluator;
use super::db::GuardrailsDb;
use super::types::{
    ApplyTo, GuardrailAction, GuardrailCheckResult, GuardrailProfile, GuardrailValidationResult,
    GuardrailsConfig, RequestContext,
};

/// Core guardrails orchestrator.
///
/// Coordinates CEL evaluation, sampling, and Bedrock API calls
/// to validate request input and output content.
pub struct GuardrailsEngine {
    config: Arc<RwLock<GuardrailsConfig>>,
    cel: CelEvaluator,
    bedrock: BedrockGuardrailClient,
}

impl GuardrailsEngine {
    /// Create a new engine, loading initial config from the database.
    pub async fn new(db: &GuardrailsDb, enabled: bool) -> Result<Self> {
        let config = db.load_config(enabled).await?;

        let cel = CelEvaluator::new();
        // Pre-compile all CEL expressions
        for rule in &config.rules {
            if !rule.cel_expression.is_empty() {
                if let Err(e) = cel.compile(&rule.cel_expression) {
                    tracing::warn!(
                        rule_id = %rule.id,
                        expression = %rule.cel_expression,
                        error = %e,
                        "Failed to compile CEL expression for rule"
                    );
                }
            }
        }

        let bedrock = BedrockGuardrailClient::new(Duration::from_secs(10));

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            cel,
            bedrock,
        })
    }

    /// Validate input content before sending to the model.
    ///
    /// Returns `None` if guardrails are disabled or no rules match.
    /// Returns `Some(result)` with the aggregate check result otherwise.
    pub async fn validate_input(
        &self,
        content: &str,
        ctx: &RequestContext,
    ) -> Result<Option<GuardrailCheckResult>> {
        self.validate(content, ctx, "INPUT").await
    }

    /// Validate output content from the model before returning to the client.
    ///
    /// Returns `None` if guardrails are disabled or no rules match.
    /// Returns `Some(result)` with the aggregate check result otherwise.
    pub async fn validate_output(
        &self,
        content: &str,
        ctx: &RequestContext,
    ) -> Result<Option<GuardrailCheckResult>> {
        self.validate(content, ctx, "OUTPUT").await
    }

    /// Reload configuration from the database.
    ///
    /// Call after any CRUD mutation on profiles or rules.
    pub async fn reload(&self, db: &GuardrailsDb, enabled: bool) -> Result<()> {
        let new_config = db.load_config(enabled).await?;

        // Clear and re-compile CEL cache
        self.cel.clear_cache();
        for rule in &new_config.rules {
            if !rule.cel_expression.is_empty() {
                if let Err(e) = self.cel.compile(&rule.cel_expression) {
                    tracing::warn!(
                        rule_id = %rule.id,
                        expression = %rule.cel_expression,
                        error = %e,
                        "Failed to compile CEL expression during reload"
                    );
                }
            }
        }

        let mut config = self.config.write().await;
        *config = new_config;

        tracing::info!("Guardrails configuration reloaded");
        Ok(())
    }

    /// Access the shared Bedrock client (e.g. for the test endpoint).
    pub fn bedrock_client(&self) -> &BedrockGuardrailClient {
        &self.bedrock
    }

    /// Core validation logic shared by validate_input and validate_output.
    async fn validate(
        &self,
        content: &str,
        ctx: &RequestContext,
        source: &str,
    ) -> Result<Option<GuardrailCheckResult>> {
        let config = self.config.read().await.clone();
        // Lock released — config is an owned snapshot

        // Short-circuit if disabled
        if !config.enabled {
            return Ok(None);
        }

        let start = Instant::now();

        // Determine which direction we're checking
        let is_input = source == "INPUT";

        // Filter rules by apply_to and enabled
        let matching_rules: Vec<_> = config
            .rules
            .iter()
            .filter(|rule| {
                if !rule.enabled {
                    return false;
                }
                match &rule.apply_to {
                    ApplyTo::Input => is_input,
                    ApplyTo::Output => !is_input,
                    ApplyTo::Both => true,
                }
            })
            .collect();

        if matching_rules.is_empty() {
            return Ok(None);
        }

        // Build profile lookup
        let profiles: std::collections::HashMap<uuid::Uuid, &GuardrailProfile> =
            config.profiles.iter().map(|p| (p.id, p)).collect();

        let mut all_results = Vec::new();

        for rule in &matching_rules {
            // Evaluate CEL expression — skip non-matching rules
            match self.cel.evaluate(&rule.cel_expression, ctx) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    tracing::warn!(
                        rule_id = %rule.id,
                        error = %e,
                        "CEL evaluation failed, skipping rule"
                    );
                    continue;
                }
            }

            // Check sampling rate
            if rule.sampling_rate < 100 {
                let roll: i16 = rand::random::<u8>() as i16 % 100;
                if roll >= rule.sampling_rate {
                    continue;
                }
            }

            // Get enabled profiles linked to this rule
            let rule_profiles: Vec<&GuardrailProfile> = rule
                .profile_ids
                .iter()
                .filter_map(|pid| profiles.get(pid).copied())
                .filter(|p| p.enabled)
                .collect();

            if rule_profiles.is_empty() {
                continue;
            }

            // Call Bedrock for all linked profiles concurrently
            let timeout = Duration::from_millis(rule.timeout_ms as u64);
            let futures: Vec<_> = rule_profiles
                .iter()
                .map(|profile| {
                    let content = content.to_string();
                    let source = source.to_string();
                    let profile = (*profile).clone();
                    let bedrock = &self.bedrock;
                    async move {
                        let call_start = Instant::now();
                        let result = tokio::time::timeout(
                            timeout,
                            bedrock.apply_guardrail(&profile, &content, &source),
                        )
                        .await;
                        let processing_time_ms = call_start.elapsed().as_millis() as u64;
                        (profile.id, result, processing_time_ms)
                    }
                })
                .collect();

            let results = futures::future::join_all(futures).await;

            for (profile_id, result, processing_time_ms) in results {
                match result {
                    Ok(Ok(response)) => {
                        all_results.push(GuardrailValidationResult {
                            rule_id: rule.id,
                            profile_id,
                            action: response.action,
                            violations: response.violations,
                            processing_time_ms,
                        });
                    }
                    Ok(Err(e)) => {
                        tracing::error!(
                            rule_id = %rule.id,
                            profile_id = %profile_id,
                            error = %e,
                            "Bedrock guardrail call failed"
                        );
                    }
                    Err(_) => {
                        tracing::error!(
                            rule_id = %rule.id,
                            profile_id = %profile_id,
                            timeout_ms = rule.timeout_ms,
                            "Bedrock guardrail call timed out"
                        );
                    }
                }
            }
        }

        if all_results.is_empty() {
            return Ok(None);
        }

        // Aggregate results: any INTERVENED → block; any REDACTED → warn; else pass
        let overall_action = if all_results
            .iter()
            .any(|r| r.action == GuardrailAction::Intervened)
        {
            GuardrailAction::Intervened
        } else if all_results
            .iter()
            .any(|r| r.action == GuardrailAction::Redacted)
        {
            GuardrailAction::Redacted
        } else {
            GuardrailAction::None
        };

        let passed = overall_action == GuardrailAction::None;
        let total_processing_time_ms = start.elapsed().as_millis() as u64;

        Ok(Some(GuardrailCheckResult {
            passed,
            action: overall_action,
            results: all_results,
            total_processing_time_ms,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_action_intervened_wins() {
        let results = vec![
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::None,
                violations: vec![],
                processing_time_ms: 10,
            },
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::Intervened,
                violations: vec![],
                processing_time_ms: 20,
            },
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::Redacted,
                violations: vec![],
                processing_time_ms: 15,
            },
        ];

        let overall = if results
            .iter()
            .any(|r| r.action == GuardrailAction::Intervened)
        {
            GuardrailAction::Intervened
        } else if results
            .iter()
            .any(|r| r.action == GuardrailAction::Redacted)
        {
            GuardrailAction::Redacted
        } else {
            GuardrailAction::None
        };

        assert_eq!(overall, GuardrailAction::Intervened);
    }

    #[test]
    fn test_aggregate_action_redacted_when_no_intervened() {
        let results = vec![
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::None,
                violations: vec![],
                processing_time_ms: 10,
            },
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::Redacted,
                violations: vec![],
                processing_time_ms: 20,
            },
        ];

        let overall = if results
            .iter()
            .any(|r| r.action == GuardrailAction::Intervened)
        {
            GuardrailAction::Intervened
        } else if results
            .iter()
            .any(|r| r.action == GuardrailAction::Redacted)
        {
            GuardrailAction::Redacted
        } else {
            GuardrailAction::None
        };

        assert_eq!(overall, GuardrailAction::Redacted);
    }

    #[test]
    fn test_aggregate_action_none_when_all_pass() {
        let results = vec![
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::None,
                violations: vec![],
                processing_time_ms: 10,
            },
            GuardrailValidationResult {
                rule_id: uuid::Uuid::new_v4(),
                profile_id: uuid::Uuid::new_v4(),
                action: GuardrailAction::None,
                violations: vec![],
                processing_time_ms: 15,
            },
        ];

        let overall = if results
            .iter()
            .any(|r| r.action == GuardrailAction::Intervened)
        {
            GuardrailAction::Intervened
        } else if results
            .iter()
            .any(|r| r.action == GuardrailAction::Redacted)
        {
            GuardrailAction::Redacted
        } else {
            GuardrailAction::None
        };

        assert_eq!(overall, GuardrailAction::None);
    }
}
