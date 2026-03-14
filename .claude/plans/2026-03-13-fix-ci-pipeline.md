# Fix CI Pipeline

## Team Preset: `fullstack` (backend + frontend + infra)

## Wave 1: Parallel fixes (all agents work simultaneously)

### rust-backend-engineer — 46 clippy warnings + broken integration test

**Dead code (19 warnings) — add `#[allow(dead_code)]` to each:**
- `backend/src/converters/anthropic_to_gemini.rs` — `anthropic_to_gemini()`
- `backend/src/converters/anthropic_to_openai.rs` — `anthropic_to_openai()`
- `backend/src/converters/openai_to_anthropic.rs` — `openai_to_anthropic()`
- `backend/src/converters/openai_to_gemini.rs` — `openai_to_gemini()`
- `backend/src/converters/core.rs` — `extract_text()`
- `backend/src/web_ui/provider_oauth.rs:147` — `validate_domain()` (2x)
- `backend/src/web_ui/model_registry.rs:315` — `all_static_models()`
- `backend/src/web_ui/copilot_auth.rs` — `refresh_in` field in CopilotTokenRow
- `backend/src/web_ui/google_auth.rs` — `device_code` in CopilotDevicePending
- `backend/src/web_ui/qwen_auth.rs` — `device_code` in QwenDevicePending
- `backend/src/routes/state.rs:26` — `refresh_token` in UserKiroCreds
- `backend/src/guardrails/bedrock.rs:53` — `output_text` in BedrockGuardrailResponse
- `backend/src/providers/types.rs:63` — `provider` in ProviderCredentials
- `backend/src/mcp/mod.rs` — `new_without_db()`, `send_request()`, `execute_tool()`, `get_all_tools_jsonrpc()`, `call_tool_jsonrpc()`
- `backend/src/providers/traits.rs` — `id()`, `is_kiro_model()`
- `backend/src/providers/kiro.rs` — unused methods
- `backend/src/mcp/transport/*.rs` — `is_connected()`, `http_client()`

**Complexity (8 warnings):**
- `backend/src/web_ui/config_db.rs` — add type aliases for 6 complex tuple types (lines 1422, 1906, 1937, 2111, 2191, 2253)
- `backend/src/web_ui/config_db.rs:2148` — add `#[allow(clippy::too_many_arguments)]` to `upsert_copilot_tokens()`
- `backend/src/converters/core.rs:991` — `.map_or(false, ...)` → `.is_some_and(...)`

**Test code (5 warnings):**
- `backend/src/streaming/mod.rs:1139` — move `#[cfg(test)] mod tests` to end of file
- `backend/src/thinking_parser.rs:421` — `assert_eq!(x, true)` → `assert!(x)`
- `backend/src/guardrails/engine.rs:284,327,363` — `vec![...]` → `[...]`

**Style (3+3 warnings):**
- `backend/src/error.rs:115` — add `#[allow(clippy::enum_variant_names)]` on ProviderApiError
- `backend/src/web_ui/api_keys.rs:317` — `extract_prefix(&key)` → `extract_prefix(key)`
- `backend/src/providers/qwen.rs:189` — `.or_insert_with(VecDeque::new)` → `.or_default()`
- `backend/src/web_ui/copilot_auth.rs:501` — `.map(...).flatten()` → `.and_then(...)`
- `backend/src/http_client.rs:277-279` — manual range → `(x..=y).contains(&z)`

**Integration test (exit code 101):**
- `backend/tests/integration_test.rs` — crate renamed: `kiro_gateway::` → `harbangan::`
- Remove `metrics` and `log_buffer` fields from AppState literal (lines 133-134)
- Remove `MetricsCollector` import (line 25)
- Add fields: `provider_registry`, `providers`, `provider_oauth_pending`, `token_exchanger`
- Reference `backend/src/routes/state.rs` for current AppState struct definition

### react-frontend-engineer — 2 lint errors + 2 warnings

- `frontend/src/components/Toast.tsx` — move `useToast()` hook (lines 17-19) into new file `frontend/src/components/useToast.ts`, keep `ToastProvider` in Toast.tsx
- Update all imports of `useToast` from `'../components/Toast'` → `'../components/useToast'` across ~10 page files
- `frontend/src/pages/Config.tsx:138-145` — move `loadHistory()` declaration before the `useEffect` that calls it; add `showToast` to deps array
- `frontend/src/pages/UserDetail.tsx:19` — add `showToast` to useEffect dependency array

### devops-engineer — GitHub Actions deprecation

- `.github/workflows/ci.yml` — update `actions/checkout@v4` → `@v4.2.2`, `actions/setup-node@v4` → `@v4.3.0`, `Swatinem/rust-cache@v2` → `@v2.7.7`

## Wave 2: Verification

```bash
cd backend && cargo fmt --check && cargo clippy --all-targets && cargo test --lib && cargo test --features test-utils
cd frontend && npm run lint && npm run build
```
