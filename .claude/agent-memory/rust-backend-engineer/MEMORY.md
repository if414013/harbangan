# Rust Backend Engineer — Agent Memory

## Key Patterns & Pitfalls

### serde rename for enum variants
- `rename_all = "snake_case"` converts `OpenAI` → `open_a_i` (wrong)
- Use per-variant `#[serde(rename = "openai")]` for variants with acronyms

### Module declaration
- All modules must be declared in `lib.rs` (not just `main.rs`) for test crate access
- Tests use the library crate — missing from lib.rs = "module not found" in tests

### async_stream
- `async-stream` must be in non-optional `[dependencies]` (not under a feature flag)
- `use futures::stream::{Stream, StreamExt}` must be in scope for `.next()` in stream! macro
- `chunk.as_ref()` (not `&chunk`) for `std::str::from_utf8` on `Bytes`

### Model types
- `ChatMessage.content` is `Option<serde_json::Value>` (string or array or null)
- `AnthropicMessage.content` is `serde_json::Value` (string or array, not Option)
- `AnthropicMessagesRequest.max_tokens` is `i32` (not u32)
- `AnthropicMessagesRequest.system` is `Option<serde_json::Value>` (string or block array)

### AppState
- Every `AppState { ... }` struct literal must include ALL fields — check `main.rs`, `routes/mod.rs` tests, `middleware/mod.rs` tests, `web_ui/google_auth.rs` tests
- After adding new AppState fields, grep for `mcp_manager: None,` to find all test helpers

### ProviderRegistry pattern
- `get_user_provider_key` returns `Result<Option<(String, String, String)>>` (api_key, key_prefix, label)
- Extract just the key: `if let Ok(Some((api_key, _, _))) = db.get_user_provider_key(...)`

## Multi-Provider Architecture (implemented in Phases 1-4)

### Files created
- `backend/src/providers/{mod,types,traits,key_detection,kiro,anthropic,openai,gemini,registry}.rs`
- `backend/src/streaming/sse.rs`
- `backend/src/converters/{openai_to_anthropic,anthropic_to_openai,openai_to_gemini,anthropic_to_gemini,gemini_to_openai,gemini_to_anthropic}.rs`
- `backend/src/web_ui/provider_keys.rs`

### Routing logic (routes/mod.rs)
- After validating the request, call `state.provider_registry.resolve_provider(user_id, model, db)`
- If result != Kiro → early return via `handle_direct_openai` or `handle_direct_anthropic`
- Kiro path unchanged — existing pipeline untouched
- Cache invalidated via `state.provider_registry.invalidate(user_id)` in provider_keys handlers

### Provider model prefix mapping
- `claude-*` → Anthropic
- `gpt-*`, `o1-*`, `o3-*`, `o4-*`, `chatgpt-*` → OpenAI
- `gemini-*` → Gemini
- everything else → Kiro (default)

### Response format conversion (non-streaming)
- OpenAI endpoint + Anthropic provider: `anthropic_response_to_openai(model, body)`
- OpenAI endpoint + Gemini provider: `gemini_to_openai::gemini_to_openai(model, body)`
- Anthropic endpoint + OpenAI provider: `openai_response_to_anthropic(model, body)`
- Anthropic endpoint + Gemini provider: `gemini_to_anthropic::gemini_to_anthropic(model, body)`
- Same-format: no conversion needed

## Test Count Milestones
- After Phase 1-2: 461 tests
- After Phase 3: 520 tests (added 59 converter tests)
- After Phase 4: 530 tests (added 10 registry tests)
