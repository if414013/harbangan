# Plan: Cache Tokens & Redacted Thinking (#109, #110, #112, #113, #121)

## Context

Five open issues remain from the LiteLLM conversion gap analysis. They fall into two independent tracks:

- **Track A (Cache Tokens)**: #109, #113, #121 — Anthropic/OpenAI APIs return cache token usage that Harbangan silently drops, breaking cost visibility
- **Track B (Redacted Thinking)**: #110, #112 — Anthropic returns `redacted_thinking` content blocks and `signature_delta` streaming events that Harbangan can't deserialize, breaking multi-turn thinking replay

Both tracks are **backend-only** — no frontend, no DB migration, no infra changes.

## Consultation Summary

Three parallel Explore agents investigated the codebase:

1. **Cache Token Agent**: Found all 6 usage struct construction sites across converters and streaming. Confirmed zero cache token references exist anywhere. Identified that cross-format streaming captures `AnthropicUsage` in `MessageStart` events — the translator needs state to carry cache fields forward.

2. **Redacted Thinking Agent**: Found `ContentBlock` has 5 variants (no `RedactedThinking`), `Delta` has 3 variants (no `SignatureDelta`). Confirmed thinking_parser.rs handles XML tags only (irrelevant — `RedactedThinking` is a JSON content block). Cross-format streaming maps `ThinkingDelta` → `reasoning_content` but has no redacted/signature handling.

3. **Test Agent**: Model files have no test modules (pure data structs). Converter files have 148 tests total. Test pattern: `#[cfg(test)] mod tests`, naming `test_<fn>_<scenario>`.

## File Manifest

| File | Action | Track | Wave |
|------|--------|-------|------|
| `backend/src/models/openai.rs` | modify | A | 1 |
| `backend/src/models/anthropic.rs` | modify | A+B | 1 |
| `backend/src/converters/kiro_to_openai.rs` | modify | A | 2 |
| `backend/src/converters/kiro_to_anthropic.rs` | modify | A | 2 |
| `backend/src/streaming/cross_format.rs` | modify | A+B | 2 |

## Wave 1: Model Types (no dependencies)

### 1.1 — Add cache token fields to usage models (#113)

**`backend/src/models/openai.rs`**:
- Add `PromptTokensDetails` struct with `cached_tokens: Option<i32>`
- Add `prompt_tokens_details: Option<PromptTokensDetails>` to `ChatCompletionUsage` (line ~183)

**`backend/src/models/anthropic.rs`**:
- Add `cache_creation_input_tokens: Option<i32>` and `cache_read_input_tokens: Option<i32>` to `AnthropicUsage` (line ~159), both with `#[serde(skip_serializing_if = "Option::is_none")]`

### 1.2 — Add redacted_thinking to Anthropic models (#112)

**`backend/src/models/anthropic.rs`**:
- Add `RedactedThinking { data: String }` variant to `ContentBlock` enum (line ~10)
- Add `SignatureDelta { signature: String }` variant to `Delta` enum (line ~231)

### 1.3 — Fix all struct construction sites (compilation)

Every place that constructs `AnthropicUsage` or `ChatCompletionUsage` must include the new fields:

| File | Lines | Add |
|------|-------|-----|
| `converters/kiro_to_openai.rs` | 94, 101 | `prompt_tokens_details: None` |
| `converters/kiro_to_anthropic.rs` | 61, 66 | `cache_creation_input_tokens: None, cache_read_input_tokens: None` |
| `streaming/cross_format.rs` | ~68, ~347 | Both sets of new fields |

### Tests (Wave 1)

Add serde round-trip tests in `models/anthropic.rs` and `models/openai.rs`:
- Serialize `AnthropicUsage` with cache fields → JSON includes them
- Deserialize JSON without cache fields → `None` (backward compat)
- Serialize/deserialize `RedactedThinking` content block
- Serialize/deserialize `SignatureDelta` delta

## Wave 2: Converters & Streaming (depends on Wave 1)

### 2.1 — Map cache tokens in cross-format streaming (#121)

**`backend/src/streaming/cross_format.rs`**:

Anthropic → OpenAI translator:
- Capture `AnthropicUsage` from `MessageStart` event into translator state (new field: `start_usage: Option<AnthropicUsage>`)
- When emitting final `ChatCompletionUsage` in `MessageDelta` handler (~line 347), populate `prompt_tokens_details.cached_tokens` from captured `cache_read_input_tokens`

OpenAI → Anthropic translator:
- When constructing `AnthropicUsage` for `message_start` event (~line 68), if source `ChatCompletionUsage` has `prompt_tokens_details.cached_tokens`, set `cache_read_input_tokens`

### 2.2 — Handle redacted thinking in streaming (#110)

**`backend/src/streaming/cross_format.rs`**:

Anthropic → OpenAI translator:
- Add match arm for `Delta::SignatureDelta { .. }` → `None` (skip, no OpenAI equivalent — signatures are Anthropic-specific for multi-turn replay)
- `RedactedThinking` content blocks flow through non-streaming responses only — no streaming handler needed (they appear as complete blocks, not deltas)

### Tests (Wave 2)

Extend existing streaming translator tests in `cross_format.rs`:
- Test `SignatureDelta` is silently skipped in Anthropic→OpenAI translation
- Test cache token passthrough in `MessageStart` usage events
- Test cache tokens appear in final OpenAI usage chunk

## Out of Scope

1. **DB schema for cache tokens** — `usage_records` table doesn't track cache tokens. Separate migration issue.
2. **Cache-aware cost calculation** — `cost.rs` uses flat pricing. Separate concern.
3. **Kiro format cache tokens** — Kiro API doesn't expose cache fields. Fields set to `None` in Kiro converters.
4. **thinking_parser.rs changes** — Handles XML tag parsing for fake reasoning. `RedactedThinking` is a JSON content block, no parser involvement.

## Verification

```bash
cd backend && cargo clippy --all-targets   # zero warnings
cd backend && cargo fmt --check            # no diffs
cd backend && cargo test --lib             # all tests pass
```

Specific test commands:
```bash
cargo test --lib models::                  # new serde tests
cargo test --lib converters::              # updated converter tests
cargo test --lib streaming::cross_format:: # updated streaming tests
```

## Recommended Execution

```
/team-implement --preset backend-feature
```

Team: 1 `rust-backend-engineer` — all changes are backend-only, single-service. The engineer writes tests inline with implementation.
