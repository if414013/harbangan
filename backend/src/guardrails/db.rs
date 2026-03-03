use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use super::types::{ApplyTo, GuardrailProfile, GuardrailRule, GuardrailsConfig};

/// Database access layer for guardrails tables.
///
/// Wraps the same PgPool used by ConfigDb — no separate connection.
pub struct GuardrailsDb {
    pool: PgPool,
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
impl GuardrailsDb {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // ── Profiles ─────────────────────────────────────────────────────

    /// List all guardrail profiles.
    pub async fn list_profiles(&self) -> Result<Vec<GuardrailProfile>> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            bool,
            String,
            String,
            String,
            String,
            String,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, provider_name, enabled, guardrail_id, guardrail_version,
                    region, access_key, secret_key, created_at, updated_at
             FROM guardrail_profiles
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list guardrail profiles")?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    provider_name,
                    enabled,
                    guardrail_id,
                    guardrail_version,
                    region,
                    access_key,
                    secret_key,
                    created_at,
                    updated_at,
                )| GuardrailProfile {
                    id,
                    name,
                    provider_name,
                    enabled,
                    guardrail_id,
                    guardrail_version,
                    region,
                    access_key,
                    secret_key,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }

    /// Get a single profile by ID.
    pub async fn get_profile(&self, id: Uuid) -> Result<Option<GuardrailProfile>> {
        let row: Option<(
            Uuid,
            String,
            String,
            bool,
            String,
            String,
            String,
            String,
            String,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, provider_name, enabled, guardrail_id, guardrail_version,
                    region, access_key, secret_key, created_at, updated_at
             FROM guardrail_profiles WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get guardrail profile")?;

        Ok(row.map(
            |(
                id,
                name,
                provider_name,
                enabled,
                guardrail_id,
                guardrail_version,
                region,
                access_key,
                secret_key,
                created_at,
                updated_at,
            )| GuardrailProfile {
                id,
                name,
                provider_name,
                enabled,
                guardrail_id,
                guardrail_version,
                region,
                access_key,
                secret_key,
                created_at,
                updated_at,
            },
        ))
    }

    /// Create a new guardrail profile.
    ///
    /// TODO: `access_key` and `secret_key` are stored as plaintext in the database.
    /// In production, these should be encrypted at rest (e.g., using a KMS envelope
    /// encryption scheme) and decrypted only when needed for API calls.
    pub async fn create_profile(
        &self,
        name: &str,
        provider_name: &str,
        enabled: bool,
        guardrail_id: &str,
        guardrail_version: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<GuardrailProfile> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        sqlx::query(
            "INSERT INTO guardrail_profiles
                (id, name, provider_name, enabled, guardrail_id, guardrail_version,
                 region, access_key, secret_key, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(id)
        .bind(name)
        .bind(provider_name)
        .bind(enabled)
        .bind(guardrail_id)
        .bind(guardrail_version)
        .bind(region)
        .bind(access_key)
        .bind(secret_key)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create guardrail profile")?;

        Ok(GuardrailProfile {
            id,
            name: name.to_string(),
            provider_name: provider_name.to_string(),
            enabled,
            guardrail_id: guardrail_id.to_string(),
            guardrail_version: guardrail_version.to_string(),
            region: region.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an existing profile. Returns true if a row was updated.
    pub async fn update_profile(
        &self,
        id: Uuid,
        name: &str,
        provider_name: &str,
        enabled: bool,
        guardrail_id: &str,
        guardrail_version: &str,
        region: &str,
        access_key: &str,
        secret_key: Option<&str>,
    ) -> Result<bool> {
        let result = if let Some(sk) = secret_key {
            sqlx::query(
                "UPDATE guardrail_profiles
                 SET name = $2, provider_name = $3, enabled = $4, guardrail_id = $5,
                     guardrail_version = $6, region = $7, access_key = $8,
                     secret_key = $9, updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(name)
            .bind(provider_name)
            .bind(enabled)
            .bind(guardrail_id)
            .bind(guardrail_version)
            .bind(region)
            .bind(access_key)
            .bind(sk)
            .execute(&self.pool)
            .await
        } else {
            // Omit secret_key update — keep existing value
            sqlx::query(
                "UPDATE guardrail_profiles
                 SET name = $2, provider_name = $3, enabled = $4, guardrail_id = $5,
                     guardrail_version = $6, region = $7, access_key = $8,
                     updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(name)
            .bind(provider_name)
            .bind(enabled)
            .bind(guardrail_id)
            .bind(guardrail_version)
            .bind(region)
            .bind(access_key)
            .execute(&self.pool)
            .await
        }
        .context("Failed to update guardrail profile")?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete a profile by ID. Returns true if a row was deleted.
    pub async fn delete_profile(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM guardrail_profiles WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete guardrail profile")?;

        Ok(result.rows_affected() > 0)
    }

    // ── Rules ────────────────────────────────────────────────────────

    /// List all guardrail rules, including their linked profile IDs.
    pub async fn list_rules(&self) -> Result<Vec<GuardrailRule>> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            bool,
            String,
            String,
            i16,
            i32,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, description, enabled, cel_expression, apply_to,
                    sampling_rate, timeout_ms, created_at, updated_at
             FROM guardrail_rules
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list guardrail rules")?;

        let mut rules: Vec<GuardrailRule> = rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    description,
                    enabled,
                    cel_expression,
                    apply_to,
                    sampling_rate,
                    timeout_ms,
                    created_at,
                    updated_at,
                )| GuardrailRule {
                    id,
                    name,
                    description,
                    enabled,
                    cel_expression,
                    apply_to: ApplyTo::parse_str(&apply_to),
                    sampling_rate,
                    timeout_ms,
                    profile_ids: Vec::new(),
                    created_at,
                    updated_at,
                },
            )
            .collect();

        // Load profile associations
        let associations: Vec<(Uuid, Uuid)> =
            sqlx::query_as("SELECT rule_id, profile_id FROM guardrail_rule_profiles")
                .fetch_all(&self.pool)
                .await
                .context("Failed to load rule-profile associations")?;

        for (rule_id, profile_id) in associations {
            if let Some(rule) = rules.iter_mut().find(|r| r.id == rule_id) {
                rule.profile_ids.push(profile_id);
            }
        }

        Ok(rules)
    }

    /// Get a single rule by ID, including its profile IDs.
    pub async fn get_rule(&self, id: Uuid) -> Result<Option<GuardrailRule>> {
        let row: Option<(
            Uuid,
            String,
            String,
            bool,
            String,
            String,
            i16,
            i32,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, description, enabled, cel_expression, apply_to,
                    sampling_rate, timeout_ms, created_at, updated_at
             FROM guardrail_rules WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get guardrail rule")?;

        let Some((
            id,
            name,
            description,
            enabled,
            cel_expression,
            apply_to,
            sampling_rate,
            timeout_ms,
            created_at,
            updated_at,
        )) = row
        else {
            return Ok(None);
        };

        let profile_ids: Vec<(Uuid,)> =
            sqlx::query_as("SELECT profile_id FROM guardrail_rule_profiles WHERE rule_id = $1")
                .bind(id)
                .fetch_all(&self.pool)
                .await
                .context("Failed to load rule profile associations")?;

        Ok(Some(GuardrailRule {
            id,
            name,
            description,
            enabled,
            cel_expression,
            apply_to: ApplyTo::parse_str(&apply_to),
            sampling_rate,
            timeout_ms,
            profile_ids: profile_ids.into_iter().map(|(pid,)| pid).collect(),
            created_at,
            updated_at,
        }))
    }

    /// Create a new guardrail rule with profile associations.
    pub async fn create_rule(
        &self,
        name: &str,
        description: &str,
        enabled: bool,
        cel_expression: &str,
        apply_to: &ApplyTo,
        sampling_rate: i16,
        timeout_ms: i32,
        profile_ids: &[Uuid],
    ) -> Result<GuardrailRule> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        sqlx::query(
            "INSERT INTO guardrail_rules
                (id, name, description, enabled, cel_expression, apply_to,
                 sampling_rate, timeout_ms, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(enabled)
        .bind(cel_expression)
        .bind(apply_to.as_str())
        .bind(sampling_rate)
        .bind(timeout_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create guardrail rule")?;

        // Insert profile associations
        for pid in profile_ids {
            sqlx::query(
                "INSERT INTO guardrail_rule_profiles (rule_id, profile_id) VALUES ($1, $2)",
            )
            .bind(id)
            .bind(pid)
            .execute(&self.pool)
            .await
            .context("Failed to link rule to profile")?;
        }

        Ok(GuardrailRule {
            id,
            name: name.to_string(),
            description: description.to_string(),
            enabled,
            cel_expression: cel_expression.to_string(),
            apply_to: apply_to.clone(),
            sampling_rate,
            timeout_ms,
            profile_ids: profile_ids.to_vec(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an existing rule and its profile associations. Returns true if updated.
    pub async fn update_rule(
        &self,
        id: Uuid,
        name: &str,
        description: &str,
        enabled: bool,
        cel_expression: &str,
        apply_to: &ApplyTo,
        sampling_rate: i16,
        timeout_ms: i32,
        profile_ids: &[Uuid],
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE guardrail_rules
             SET name = $2, description = $3, enabled = $4, cel_expression = $5,
                 apply_to = $6, sampling_rate = $7, timeout_ms = $8, updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(enabled)
        .bind(cel_expression)
        .bind(apply_to.as_str())
        .bind(sampling_rate)
        .bind(timeout_ms)
        .execute(&self.pool)
        .await
        .context("Failed to update guardrail rule")?;

        if result.rows_affected() == 0 {
            return Ok(false);
        }

        // Replace profile associations
        sqlx::query("DELETE FROM guardrail_rule_profiles WHERE rule_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to clear rule profile associations")?;

        for pid in profile_ids {
            sqlx::query(
                "INSERT INTO guardrail_rule_profiles (rule_id, profile_id) VALUES ($1, $2)",
            )
            .bind(id)
            .bind(pid)
            .execute(&self.pool)
            .await
            .context("Failed to link rule to profile")?;
        }

        Ok(true)
    }

    /// Delete a rule by ID. Returns true if deleted.
    pub async fn delete_rule(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM guardrail_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete guardrail rule")?;

        Ok(result.rows_affected() > 0)
    }

    // ── Config snapshot ──────────────────────────────────────────────

    /// Load an in-memory snapshot of all guardrails data.
    pub async fn load_config(&self, enabled: bool) -> Result<GuardrailsConfig> {
        let profiles = self.list_profiles().await?;
        let rules = self.list_rules().await?;

        Ok(GuardrailsConfig {
            enabled,
            rules,
            profiles,
        })
    }
}
