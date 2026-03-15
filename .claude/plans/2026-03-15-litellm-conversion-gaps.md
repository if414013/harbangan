# Plan: LiteLLM Conversion Gaps - API Compatibility Fixes

**Date**: 2026-03-15
**Based on**: Harbangan v1.0.7 vs LiteLLM v1.72+ comparison
**Goal**: Close critical API compatibility gaps to match LiteLLM's conversion completeness

---

## Consultation Summary

### Backend Findings (from rust-backend-engineer)
- **Converters**: 6 converter files handle cross-format conversion, but P0 parameters (`tool_choice`, `tools`, `response_format`) are hardcoded to `None`
- **Models**: `ChatCompletionRequest` has `tool_choice` field but it's not used in converters. Missing `response_format`, `reasoning_effort` fields
- **Streaming**: Kiro-to-OpenAI/Anthropic streaming exists, but no schema translation for cross-format SSE (OpenAI endpoint → Anthropic provider returns wrong schema)
- **Location**: `backend/src/converters/` for converters, `backend/src/models/` for types, `backend/src/streaming/` for SSE

### Frontend Findings
- No frontend changes required - all gaps are backend API layer fixes

### Infrastructure Findings
- No infrastructure changes required
- No new dependencies needed (all conversions are pure Rust transformations)

---

## File Manifest

| File | Action | Owner | Wave |
|------|--------|-------|------|
| `backend/src/models/openai.rs` | Modify - add `response_format`, `reasoning_effort` fields | rust-backend-engineer | 1 |
| `backend/src/models/anthropic.rs` | Modify - add `RedactedThinking` to ContentBlock, `SignatureDelta` to Delta | rust-backend-engineer | 1 |
| `backend/src/converters/anthropic_to_openai.rs` | Modify - add tool_choice, tools, response_format mapping | rust-backend-engineer | 2 |
| `backend/src/converters/openai_to_anthropic.rs` | Modify - add tool_choice, tools, response_format mapping | rust-backend-engineer | 2 |
| `backend/src/converters/anthropic_to_kiro.rs` | Modify - add tool_choice, response_format mapping | rust-backend-engineer | 2 |
| `backend/src/converters/openai_to_kiro.rs` | Modify - add tool_choice, response_format, reasoning_effort mapping | rust-backend-engineer | 2 |
| `backend/src/converters/kiro_to_openai.rs` | Modify - add response_format, reasoning_effort in response | rust-backend-engineer | 2 |
| `backend/src/converters/kiro_to_anthropic.rs` | Modify - add redacted_thinking handling | rust-backend-engineer | 2 |
| `backend/src/streaming/mod.rs` | Modify - add cross-format SSE schema translation | rust-backend-engineer | 3 |
| `backend/src/streaming/cross_format.rs` | Create - streaming translation utilities | rust-backend-engineer | 3 |
| `backend/src/converters/core.rs` | Modify - update shared conversion utilities | rust-backend-engineer | 2 |
| `backend/src/routes/anthropic.rs` | Modify - integrate streaming translation | rust-backend-engineer | 3 |
| `backend/src/routes/openai.rs` | Modify - integrate streaming translation | rust-backend-engineer | 3 |
| `backend/src/usage/mod.rs` | Modify - add cache token fields | rust-backend-engineer | 4 |

---

## Wave 1: Foundation - Model Types

**Goal**: Add missing fields to request/response models

### Task 1.1: Add `response_format` to OpenAI models
- **File**: `backend/src/models/openai.rs`
- **Changes**:
  - Add `ResponseFormat` enum with variants: `Text`, `JsonObject`, `JsonSchema { json_schema: JsonSchema }`
  - Add `JsonSchema` struct with `name`, `description`, `schema`, `strict` fields
  - Add `response_format: Option<ResponseFormat>` to `ChatCompletionRequest`
  - Add `reasoning_effort: Option<String>` to `ChatCompletionRequest` (values: "low", "medium", "high")
- **Pattern**: Follow existing serde patterns with `#[serde(rename_all = "snake_case")]`

### Task 1.2: Add redacted thinking to Anthropic models
- **File**: `backend/src/models/anthropic.rs`
- **Changes**:
  - Add `RedactedThinking { data: String }` variant to `ContentBlock` enum
  - Add `SignatureDelta { signature: String }` variant to `Delta` enum
  - Ensure `#[serde(rename_all = "snake_case")]` on ContentBlock

### Task 1.3: Expand usage models with cache tokens
- **File**: `backend/src/models/openai.rs` and `backend/src/models/anthropic.rs`
- **Changes**:
  - Add `PromptTokensDetails { cached_tokens: i32 }` struct
  - Add `prompt_tokens_details: Option<PromptTokensDetails>` to `ChatCompletionUsage`
  - Add `cache_creation_input_tokens: Option<i32>`, `cache_read_input_tokens: Option<i32>` to `AnthropicUsage`

**Verification**: `cd backend && cargo clippy --all-targets` - zero warnings

---

## Wave 2: Converter Logic - Request/Response Mapping

**Goal**: Implement bidirectional parameter conversion in all 6 converters

### Task 2.1: tool_choice mapping (GAP-001)
- **Files**: `anthropic_to_openai.rs`, `openai_to_anthropic.rs`, `anthropic_to_kiro.rs`, `openai_to_kiro.rs`
- **Mapping**:
  ```
  OpenAI → Anthropic:
    "auto" → {"type": "auto"}
    "none" → drop tools param entirely
    "required" → {"type": "any"}
    {"type":"function","function":{"name":"X"}} → {"type":"tool","name":"X"}
    parallel_tool_calls: false → disable_parallel_tool_use: true

  Anthropic → OpenAI:
    {"type": "auto"} → "auto"
    {"type": "any"} → "required"
    {"type": "tool", "name": "X"} → {"type":"function","function":{"name":"X"}}
    disable_parallel_tool_use: true → parallel_tool_calls: false
  ```
- **Pattern**: Create helper function `map_tool_choice()` in `core.rs` for reuse

### Task 2.2: response_format mapping (GAP-003)
- **Files**: `openai_to_anthropic.rs`, `anthropic_to_openai.rs`, `openai_to_kiro.rs`
- **Mapping**:
  ```
  OpenAI → Anthropic (Sonnet 4.5+/Opus 4.1+):
    json_schema → output_format with $ref/$defs resolution
    Filter unsupported constraints (maxItems, minimum, etc.) into description

  OpenAI → Anthropic (older models):
    json_schema → tool call fallback with constrained schema

  Anthropic → OpenAI:
    output_format → json_schema (reverse mapping)
  ```
- **Pattern**: Create helper `map_response_format()` in `core.rs`

### Task 2.3: reasoning_effort mapping (GAP-004)
- **Files**: `openai_to_anthropic.rs`, `openai_to_kiro.rs`
- **Mapping**:
  ```
  OpenAI → Anthropic:
    reasoning_effort: "low" → thinking: {type: "enabled", budget_tokens: 1000}
    reasoning_effort: "medium" → thinking: {type: "enabled", budget_tokens: 2000}
    reasoning_effort: "high" → thinking: {type: "enabled", budget_tokens: 4000}
    Drop temperature when thinking enabled
  ```
- **Pattern**: Model-aware mapping (adaptive for Claude 4.6)

### Task 2.4: tools array conversion
- **Files**: All 6 converters
- **Mapping**: Convert OpenAI `Tool` format ↔ Anthropic `AnthropicTool` format
- **Pattern**: Reuse existing tool conversion logic from Kiro converters

### Task 2.5: Redacted thinking in response
- **Files**: `kiro_to_anthropic.rs`, `kiro_to_openai.rs`
- **Changes**: Preserve and forward `redacted_thinking` blocks for multi-turn replay

**Verification**: `cd backend && cargo test --lib converters::` - all converter tests pass

---

## Wave 3: Streaming - Schema Translation (GAP-002)

**Goal**: Translate SSE events to match endpoint format

### Task 3.1: Create cross-format streaming module
- **File**: `backend/src/streaming/cross_format.rs` (NEW)
- **Functions**:
  - `translate_openai_to_anthropic_stream()` - Parse OpenAI SSE, emit Anthropic events
  - `translate_anthropic_to_openai_stream()` - Parse Anthropic SSE, emit OpenAI chunks
- **Event mappings**:
  ```
  OpenAI → Anthropic:
    data: {"choices":[{"delta":{"content":"x"}}]} → {"type":"content_block_delta","delta":{"text_delta":"x"}}
    data: {"choices":[{"delta":{"tool_calls":...}}]} → {"type":"content_block_delta","delta":{"input_json_delta":...}}
    data: {"choices":[{"finish_reason":"tool_calls"}]} → {"type":"message_delta","delta":{"stop_reason":"tool_use"}}

  Anthropic → OpenAI:
    {"type":"content_block_delta","delta":{"text_delta":"x"}} → data: {"choices":[{"delta":{"content":"x"}}]}
    {"type":"content_block_delta","delta":{"input_json_delta":...}} → data: {"choices":[{"delta":{"tool_calls":...}}]}
    {"type":"message_delta","delta":{"stop_reason":"tool_use"}} → data: {"choices":[{"finish_reason":"tool_calls"}]}
  ```

### Task 3.2: Integrate streaming translation into routes
- **Files**: `backend/src/routes/anthropic.rs`, `backend/src/routes/openai.rs`
- **Changes**:
  - Detect cross-format requests (endpoint format ≠ provider format)
  - Wrap SSE stream with translation function
  - Handle thinking deltas, signature deltas, usage events

### Task 3.3: Signature delta handling
- **File**: `backend/src/streaming/mod.rs`
- **Changes**: Add `signature_delta` event type support for thinking block signatures

**Verification**: `cd backend && cargo test --lib streaming::` - all streaming tests pass

---

## Wave 4: Usage & Cache Tokens (GAP-005)

**Goal**: Add cache token visibility to usage tracking

### Task 4.1: Expand usage models
- **File**: `backend/src/usage/mod.rs`
- **Changes**:
  - Add cache token fields to usage structs
  - Map between OpenAI `prompt_tokens_details.cached_tokens` ↔ Anthropic `cache_read_input_tokens`

### Task 4.2: Update usage normalization
- **Files**: `kiro_to_openai.rs`, `kiro_to_anthropic.rs`
- **Changes**: Extract and map cache token fields from Kiro response

**Verification**: `cd backend && cargo test --lib usage::` - all usage tests pass

---

## Interface Contracts

### tool_choice JSON Shape

```json
// OpenAI format
{"type": "auto" | "none" | "required" | "function", "function": {"name": "..."}}

// Anthropic format
{"type": "auto" | "any" | "tool", "name": "..."} | None (to drop)
```

### response_format JSON Shape

```json
// OpenAI format
{
  "type": "json_schema",
  "json_schema": {
    "name": "MySchema",
    "schema": {...},
    "strict": true
  }
}

// Anthropic format (Sonnet 4.5+)
{
  "type": "json_schema",
  "name": "MySchema",
  "schema": {...}
}
```

### Streaming Event Shapes

See Wave 3 task description for full mapping tables.

---

## Verification Commands

### Backend Quality Gates
```bash
cd backend && cargo clippy --all-targets          # Zero warnings
cd backend && cargo fmt --check                   # No diffs
cd backend && cargo test --lib                    # Zero failures
cd backend && cargo test --lib converters::       # Converter tests
cd backend && cargo test --lib streaming::        # Streaming tests
cd backend && cargo test --lib models::           # Model tests
```

### E2E Validation
```bash
cd e2e-tests && npm run test:api                  # API tests with new params
```

---

## GitHub Issues

Created on Harbangan Board (github.com/if414013/harbangan/issues):

| Issue | Title | Priority | Size | Wave | Status |
|-------|-------|----------|------|------|--------|
| #111 | backend: Add response_format and reasoning_effort to OpenAI models | P0 | S | 1 | ✅ Done |
| #112 | backend: Add redacted_thinking to Anthropic ContentBlock | P1 | S | 1 | ✅ Done |
| #113 | backend: Add cache token fields to usage models | P1 | S | 1 | ✅ Done |
| #114 | backend: Implement tool_choice bidirectional mapping | P0 | M | 2 | Backlog |
| #115 | backend: Implement response_format conversion with JSON schema resolution | P0 | M | 2 | Backlog |
| #116 | backend: Implement reasoning_effort → thinking mapping | P1 | S | 2 | Backlog |
| #117 | backend: Convert tools array in all cross-format converters | P0 | M | 2 | Backlog |
| #118 | backend: Create cross-format streaming translation module | P0 | L | 3 | Backlog |
| #119 | backend: Integrate streaming translation into Anthropic/OpenAI routes | P0 | M | 3 | Backlog |
| #120 | backend: Add signature_delta event handling | P1 | S | 3 | Backlog |
| #121 | backend: Map cache tokens between OpenAI and Anthropic formats | P1 | S | 4 | Backlog |

---

## Progress

### Wave 1: Foundation - Model Types ✅ COMPLETED

**Commit**: `a425ae3b` on `feat/litellm-conversion-gaps` (branch deleted, changes on main)

**Changes**:
- Added `ResponseFormat` enum with `Text`, `JsonObject`, `JsonSchema` variants
- Added `JsonSchema` struct with name, description, schema, strict fields
- Added `response_format` and `reasoning_effort` to `ChatCompletionRequest`
- Added `PromptTokensDetails` with `cached_tokens` to `ChatCompletionUsage`
- Updated all test fixtures in converters to include new fields
- Verified: `RedactedThinking` and `SignatureDelta` already exist in anthropic.rs
- Verified: Cache token fields already exist in `AnthropicUsage`

**Verification**:
- `cargo clippy --all-targets` - ✅ Passed
- `cargo test --lib` - ✅ 748 tests passed

### Wave 2: Converter Logic - 🔶 PARTIALLY DONE (uncommitted, does not compile)

**State**: 256 lines of uncommitted changes across 5 files. Tests and integration points written, but 5 core helper functions never implemented.

**What exists (salvageable):**
- `models/openai.rs`: Added `reasoning_effort`, `response_format` fields ✅
- `models/anthropic.rs`: Added `thinking`, `disable_parallel_tool_use` fields ✅
- `openai_to_anthropic.rs`: Converter calls helper functions, maps tool_choice/reasoning/response_format ✅
- `anthropic_to_openai.rs`: Converter calls helper functions, maps tool_choice/thinking ✅
- `core.rs`: 16 comprehensive tests for all 5 helper functions ✅

**What's missing (blocks compilation):**
- `core.rs`: 5 `pub fn` helper functions never implemented:
  1. `map_tool_choice_openai_to_anthropic(&Option<Value>, Option<bool>) -> (Option<Value>, Option<bool>)`
  2. `map_tool_choice_anthropic_to_openai(&Option<Value>, Option<bool>) -> (Option<Value>, Option<bool>)`
  3. `map_response_format_openai_to_anthropic(&Option<Value>) -> Option<Value>`
  4. `map_response_format_anthropic_to_openai(&Option<Value>) -> Option<Value>`
  5. `map_reasoning_effort_to_thinking(Option<&str>) -> (Option<Value>, bool)`

**Structural issue**: The 16 tests were placed inside the existing `#[cfg(test)] mod tests` block (starts line 1101). The helper functions must be placed BEFORE this test module as public module-level functions so they can be imported by other converter files.

---

## Revised Implementation Plan (Wave 2 completion + Waves 3-4)

### Wave 2A: Complete Core Helper Functions (IMMEDIATE)

**Goal**: Implement the 5 missing functions, get compilation passing, all 16 new tests green.

**Single task — one agent (`rust-backend-engineer`):**

1. Add 5 `pub fn` implementations to `core.rs` BEFORE the `#[cfg(test)] mod tests` block (before line 1101):
   - `map_tool_choice_openai_to_anthropic` — string/object → Anthropic format
   - `map_tool_choice_anthropic_to_openai` — reverse mapping
   - `map_response_format_openai_to_anthropic` — extract json_schema, filter `additionalProperties`
   - `map_response_format_anthropic_to_openai` — wrap back in json_schema
   - `map_reasoning_effort_to_thinking` — effort level → budget_tokens (1000/2000/4000)

2. Verify: `cargo clippy --all-targets && cargo test --lib` — zero warnings, all tests pass

**Complexity**: Small — function signatures and test expectations already defined. Pure implementation.

### Wave 2B: Remaining Converter Integration

**Goal**: Wire up remaining converters that don't yet use the helpers.

**Tasks:**
- [ ] `anthropic_to_kiro.rs` — add tool_choice mapping (currently hardcoded to None)
- [ ] `openai_to_kiro.rs` — add tool_choice, response_format, reasoning_effort mapping
- [ ] `kiro_to_openai.rs` — add response_format in response, reasoning_effort reverse mapping
- [ ] `kiro_to_anthropic.rs` — ensure redacted_thinking blocks are preserved

**Complexity**: Small-Medium — patterns established in 2A, just apply to remaining files.

### Wave 3: Streaming Schema Translation (GAP-002)

No changes from original plan. See Wave 3 section above.

### Wave 4: Usage & Cache Tokens (GAP-005)

No changes from original plan. See Wave 4 section above.

---

## Recommended Team Preset

**`/team-implement --preset backend-feature`**

All work is backend-only. Recommended team composition:
- **rust-backend-engineer** — implement 5 helper functions (Wave 2A), wire remaining converters (Wave 2B), streaming translation (Wave 3)
- **backend-qa** — verify test coverage, add edge case tests for streaming (Wave 3)

### Wave Dependencies

```
Wave 2A (5 helpers) ──► Wave 2B (remaining converters) ──► Wave 3 (Streaming) ──► Wave 4 (Usage)
     │                        │                                  │
  Implement missing      Apply helpers to                  Streaming needs
  functions, fix         kiro converters                   converter mapping
  compilation                                              functions
```

### Estimated Remaining Complexity

| Wave | Complexity | Rationale |
|------|------------|-----------|
| Wave 2A | Small | 5 functions, signatures + tests already defined |
| Wave 2B | Small-Medium | 4 converters, patterns established |
| Wave 3 | Large | Streaming state machine, bidirectional SSE translation |
| Wave 4 | Small | Additive usage fields, straightforward mapping |

---

## Risk & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Uncommitted changes conflict with main | Medium | Verify diff is clean, commit early on feature branch |
| core.rs structural issue (test module scope) | High | Place helpers BEFORE line 1101 test module, not inside it |
| Breaking existing converters | High | Run full test suite after each converter change |
| Streaming translation bugs | High | Start with non-streaming paths, add streaming incrementally |
| JSON schema resolution complexity | Medium | Start with simple passthrough, add $ref/$defs as follow-up |

---

## Success Criteria

1. **Compilation**: `cargo check` passes with zero errors (currently broken)
2. **tool_choice**: All test clients can force tool use via cross-format requests
3. **response_format**: Structured outputs work on Anthropic endpoint → OpenAI provider
4. **streaming**: Anthropic SDK clients can consume cross-format SSE without errors
5. **usage**: Cache tokens visible in API responses for models that support caching
6. **tests**: 100% of new converter paths have test coverage, all 748+ tests pass
