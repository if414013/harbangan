# Harbangan Schema Inventory (as of v25)

## Migration Version History

| Version | Description | Method |
|---------|-------------|--------|
| 1 | schema_version, config, config_history | inline in run_migrations() |
| 3 | users, sessions, user_kiro_tokens, api_keys, allowed_domains | migrate_to_v3() |
| 4 | guardrail_profiles, guardrail_rules, guardrail_rule_profiles | migrate_to_v4() |
| 5 | mcp_clients (later dropped in v16) | migrate_to_v5() |
| 6 | user_kiro_tokens: add oauth_client_id, oauth_client_secret, oauth_sso_region | migrate_to_v6() |
| 7 | user_provider_keys, model_routes | migrate_to_v7() |
| 8 | user_provider_tokens | migrate_to_v8() |
| 9 | user_copilot_tokens, user_provider_priority; model_routes CHECK += copilot | migrate_to_v9() |
| 10 | provider_tokens CHECK += qwen; add base_url; model_routes CHECK += qwen | migrate_to_v10() |
| 11 | Rename openai -> openai_codex in all provider tables | migrate_to_v11() |
| 12 | model_registry | migrate_to_v12() |
| 13 | Remove Gemini provider (data + constraints) | migrate_to_v13() |
| 14 | user_kiro_tokens: add oauth_start_url, backfill from config | migrate_to_v14() |
| 15 | users: add password_hash, totp_secret, totp_enabled, auth_method, must_change_password; totp_recovery_codes, pending_2fa_logins | migrate_to_v15() |
| 16 | Drop mcp_clients table, remove mcp_% config keys | migrate_to_v16() |
| 17 | users: add google_linked column | migrate_to_v17() |
| 18 | Seed provider OAuth client IDs into config table | migrate_to_v18() |
| 19 | usage_records table + indexes | migrate_to_v19() |
| 20 | user_provider_tokens multi-account (account_label), admin_provider_pool | migrate_to_v20() |
| 21 | Drop all provider_id CHECK constraints (validation moved to Rust) | migrate_to_v21() |
| 22 | Remove Qwen provider data from all tables | migrate_to_v22() |
| 23 | Add non-negative CHECK constraints on usage_records | migrate_to_v23() |
| 24 | model_visibility_defaults table + idx_mvd_provider index | migrate_to_v24() |
| 25 | provider_settings table, seed 4 providers as enabled | migrate_to_v25() |

## Active Tables (22 total)

### schema_version (v1)
| Column | Type | Constraints |
|--------|------|-------------|
| version | INTEGER | NOT NULL |
| applied_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |

### config (v1)
| Column | Type | Constraints |
|--------|------|-------------|
| key | TEXT | PRIMARY KEY |
| value | TEXT | NOT NULL |
| value_type | TEXT | NOT NULL DEFAULT 'string' |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| description | TEXT | |

### config_history (v1)
| Column | Type | Constraints |
|--------|------|-------------|
| id | SERIAL | PRIMARY KEY |
| key | TEXT | NOT NULL |
| old_value | TEXT | |
| new_value | TEXT | NOT NULL |
| changed_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| source | TEXT | NOT NULL DEFAULT 'web_ui' |

### users (v3, extended v15, v17)
| Column | Type | Constraints | Added |
|--------|------|-------------|-------|
| id | UUID | PRIMARY KEY | v3 |
| email | TEXT | UNIQUE NOT NULL | v3 |
| name | TEXT | NOT NULL | v3 |
| picture_url | TEXT | | v3 |
| role | TEXT | NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')) | v3 |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() | v3 |
| last_login | TIMESTAMPTZ | | v3 |
| password_hash | TEXT | | v15 |
| totp_secret | TEXT | | v15 |
| totp_enabled | BOOLEAN | NOT NULL DEFAULT FALSE | v15 |
| auth_method | TEXT | NOT NULL DEFAULT 'google' | v15 |
| must_change_password | BOOLEAN | NOT NULL DEFAULT FALSE | v15 |
| google_linked | BOOLEAN | NOT NULL DEFAULT FALSE | v17 |

### sessions (v3)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| expires_at | TIMESTAMPTZ | NOT NULL |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Index:** idx_sessions_user(user_id)

### user_kiro_tokens (v3, extended v6, v14)
| Column | Type | Constraints | Added |
|--------|------|-------------|-------|
| user_id | UUID | PRIMARY KEY FK -> users(id) ON DELETE CASCADE | v3 |
| refresh_token | TEXT | NOT NULL | v3 |
| access_token | TEXT | | v3 |
| token_expiry | TIMESTAMPTZ | | v3 |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() | v3 |
| oauth_client_id | TEXT | | v6 |
| oauth_client_secret | TEXT | | v6 |
| oauth_sso_region | TEXT | | v6 |
| oauth_start_url | TEXT | | v14 |

### api_keys (v3)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| key_hash | TEXT | UNIQUE NOT NULL |
| key_prefix | TEXT | NOT NULL |
| label | TEXT | NOT NULL DEFAULT '' |
| last_used | TIMESTAMPTZ | |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Indexes:** idx_api_keys_hash(key_hash), idx_api_keys_user(user_id)

### allowed_domains (v3)
| Column | Type | Constraints |
|--------|------|-------------|
| domain | TEXT | PRIMARY KEY |
| added_by | UUID | FK -> users(id) |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |

### guardrail_profiles (v4)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() |
| name | TEXT | NOT NULL |
| provider_name | TEXT | NOT NULL DEFAULT 'bedrock' |
| enabled | BOOLEAN | NOT NULL DEFAULT true |
| guardrail_id | TEXT | NOT NULL |
| guardrail_version | TEXT | NOT NULL DEFAULT '1' |
| region | TEXT | NOT NULL DEFAULT 'us-east-1' |
| access_key | TEXT | NOT NULL |
| secret_key | TEXT | NOT NULL |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |

### guardrail_rules (v4)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() |
| name | TEXT | NOT NULL |
| description | TEXT | NOT NULL DEFAULT '' |
| enabled | BOOLEAN | NOT NULL DEFAULT true |
| cel_expression | TEXT | NOT NULL DEFAULT '' |
| apply_to | TEXT | NOT NULL DEFAULT 'both' CHECK (IN 'input','output','both') |
| sampling_rate | SMALLINT | NOT NULL DEFAULT 100 CHECK (BETWEEN 0 AND 100) |
| timeout_ms | INTEGER | NOT NULL DEFAULT 5000 |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |

### guardrail_rule_profiles (v4)
| Column | Type | Constraints |
|--------|------|-------------|
| rule_id | UUID | NOT NULL FK -> guardrail_rules(id) ON DELETE CASCADE |
| profile_id | UUID | NOT NULL FK -> guardrail_profiles(id) ON DELETE CASCADE |
**Primary Key:** (rule_id, profile_id)

### user_provider_keys (v7)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| provider_id | TEXT | NOT NULL (no CHECK after v21) |
| api_key | TEXT | NOT NULL |
| key_prefix | TEXT | NOT NULL |
| label | TEXT | NOT NULL DEFAULT '' |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Unique:** (user_id, provider_id)

### model_routes (v7)
| Column | Type | Constraints |
|--------|------|-------------|
| model_pattern | TEXT | PRIMARY KEY |
| provider_id | TEXT | NOT NULL (no CHECK after v21) |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |

### user_provider_tokens (v8, extended v10, v20)
| Column | Type | Constraints | Added |
|--------|------|-------------|-------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() | v8 |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE | v8 |
| provider_id | TEXT | NOT NULL (no CHECK after v21) | v8 |
| access_token | TEXT | NOT NULL | v8 |
| refresh_token | TEXT | NOT NULL DEFAULT '' | v8 |
| expires_at | TIMESTAMPTZ | NOT NULL | v8 |
| email | TEXT | NOT NULL DEFAULT '' | v8 |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() | v8 |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() | v8 |
| base_url | TEXT | | v10 |
| account_label | TEXT | NOT NULL DEFAULT 'default' | v20 |
**Unique:** (user_id, provider_id, account_label) -- changed from (user_id, provider_id) in v20

### user_copilot_tokens (v9)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| github_token | TEXT | NOT NULL |
| github_username | TEXT | |
| copilot_token | TEXT | |
| copilot_plan | TEXT | |
| base_url | TEXT | |
| expires_at | TIMESTAMPTZ | |
| refresh_in | BIGINT | |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Unique:** (user_id)

### user_provider_priority (v9)
| Column | Type | Constraints |
|--------|------|-------------|
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| provider_id | TEXT | NOT NULL |
| priority | INTEGER | NOT NULL DEFAULT 0 |
**Primary Key:** (user_id, provider_id)

### model_registry (v12)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY |
| provider_id | TEXT | NOT NULL |
| model_id | TEXT | NOT NULL |
| display_name | TEXT | NOT NULL |
| prefixed_id | TEXT | NOT NULL UNIQUE |
| context_length | INTEGER | NOT NULL DEFAULT 0 |
| max_output_tokens | INTEGER | NOT NULL DEFAULT 0 |
| capabilities | JSONB | NOT NULL DEFAULT '{}' |
| enabled | BOOLEAN | NOT NULL DEFAULT true |
| source | TEXT | NOT NULL DEFAULT 'static' |
| upstream_meta | JSONB | |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Unique:** (provider_id, model_id)
**Partial Index:** idx_model_registry_enabled ON (provider_id, model_id) WHERE enabled=true

### totp_recovery_codes (v15)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| code_hash | TEXT | NOT NULL |
| used | BOOLEAN | NOT NULL DEFAULT FALSE |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Index:** idx_recovery_codes_user(user_id)

### pending_2fa_logins (v15)
| Column | Type | Constraints |
|--------|------|-------------|
| token | UUID | PRIMARY KEY |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| expires_at | TIMESTAMPTZ | NOT NULL |
**Index:** idx_pending_2fa_user(user_id)

### usage_records (v19, constraints v23)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() |
| user_id | UUID | NOT NULL FK -> users(id) ON DELETE CASCADE |
| provider_id | TEXT | NOT NULL |
| model_id | TEXT | NOT NULL |
| input_tokens | INTEGER | NOT NULL CHECK (>= 0) |
| output_tokens | INTEGER | NOT NULL CHECK (>= 0) |
| cost | DOUBLE PRECISION | NOT NULL DEFAULT 0.0 CHECK (>= 0) |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Indexes:** idx_usage_user_date(user_id, created_at), idx_usage_provider_model_date(provider_id, model_id, created_at), idx_usage_date(created_at)
**Named constraints (v23):** usage_records_input_tokens_nonnegative, usage_records_output_tokens_nonnegative, usage_records_cost_nonnegative

### admin_provider_pool (v20)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY DEFAULT gen_random_uuid() |
| provider_id | TEXT | NOT NULL (no CHECK after v21) |
| account_label | TEXT | NOT NULL DEFAULT 'pool-1' |
| api_key | TEXT | NOT NULL |
| key_prefix | TEXT | NOT NULL DEFAULT '' |
| base_url | TEXT | |
| enabled | BOOLEAN | NOT NULL DEFAULT true |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Unique:** (provider_id, account_label)

### model_visibility_defaults (v24)
| Column | Type | Constraints |
|--------|------|-------------|
| id | UUID | PRIMARY KEY |
| provider_id | TEXT | NOT NULL |
| model_id | TEXT | NOT NULL |
| created_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Unique:** (provider_id, model_id)
**Index:** idx_mvd_provider(provider_id)

### provider_settings (v25)
| Column | Type | Constraints |
|--------|------|-------------|
| provider_id | TEXT | PRIMARY KEY |
| enabled | BOOLEAN | NOT NULL DEFAULT true |
| updated_at | TIMESTAMPTZ | NOT NULL DEFAULT NOW() |
**Seeded rows:** kiro, anthropic, openai_codex, copilot (all enabled)

## Dropped Tables

### mcp_clients (created v5, dropped v16)
Was used for MCP server connections. Entire table and related config keys removed.
