# Backend Modularization Plan

## Context

The backend has 6 providers (Kiro, Anthropic, OpenAI Codex, Gemini, Copilot, Qwen) but the code isn't truly modular. While a `Provider` trait and individual provider structs exist, the actual request handling bypasses them with hardcoded match chains, provider-specific AppState fields, and ~400 lines of duplicated pipeline logic between the OpenAI and Anthropic handlers. Adding a new provider currently requires changes to 4+ files and touching `routes/mod.rs` (a 1395-line monolith). The goal is to make each provider a self-contained unit so extending, debugging, and customizing is straightforward.

## Current Problems

1. **`routes/mod.rs` (1395 lines)** — AppState, both handlers, all pipeline logic, format conversion helpers, provider dispatch match chains — all in one file
2. **Match chains instead of polymorphism** — `handle_direct_openai` and `handle_direct_anthropic` dispatch via `match provider_id { ... }` for each of the 5 non-Kiro providers
3. **Provider-specific state leaks into AppState** — `copilot_token_cache`, `copilot_device_pending`, `qwen_device_pending`, plus each provider as a separate named field (`anthropic_provider`, `openai_codex_provider`, etc.)
4. **Response format conversion in the wrong place** — `anthropic_response_to_openai()` and `openai_response_to_anthropic()` live in routes/mod.rs instead of provider modules
5. **KiroProvider is a stub** — the entire Kiro pipeline is inlined in the handlers (~200 lines each)
6. **~400 lines duplicated** between `chat_completions_handler` and `anthropic_messages_handler`

## Target Architecture

### Adding a new provider should only require:
1. Create `providers/new_provider.rs` implementing `Provider` trait
2. Add variant to `ProviderId` enum
3. Register in provider map initialization (one line in `main.rs`)
4. Add auth flow in `web_ui/` if needed

No changes to routes, handlers, or AppState struct.

### Target file structure:
```
backend/src/
├── routes/
│   ├── mod.rs          # Router setup, route registration only
│   ├── state.rs        # AppState struct + helpers
│   ├── openai.rs       # chat_completions_handler
│   ├── anthropic.rs    # anthropic_messages_handler
│   └── pipeline.rs     # Shared pipeline stages (guardrails, MCP, truncation)
├── providers/
│   ├── mod.rs          # Provider map type + registration
│   ├── traits.rs       # Enhanced Provider trait
│   ├── types.rs        # ProviderId, ProviderCredentials, ProviderContext, etc.
│   ├── registry.rs     # ProviderRegistry (resolve user→provider)
│   ├── kiro.rs         # KiroProvider (fully wired)
│   ├── anthropic.rs    # AnthropicProvider (with response normalization)
│   ├── copilot.rs      # CopilotProvider (with encapsulated state)
│   ├── gemini.rs       # GeminiProvider (with response normalization)
│   ├── openai_codex.rs # OpenAICodexProvider
│   └── qwen.rs         # QwenProvider (with encapsulated state)
```

---

## Implementation Waves

### Wave 1: Provider trait enhancement + polymorphic dispatch
**Goal:** Eliminate match chains, make dispatch polymorphic
**Risk:** Medium — changes dispatch mechanism but same behavior
**Files:** `providers/traits.rs`, `providers/mod.rs`, `providers/*.rs`, `routes/mod.rs`, `main.rs`

#### 1.1: Add response normalization to Provider trait
```rust
// providers/traits.rs — add default methods
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;

    // Existing methods (unchanged)
    async fn execute_openai(...) -> Result<ProviderResponse, ApiError>;
    async fn stream_openai(...) -> Result<Pin<Box<dyn Stream<...>>>, ApiError>;
    async fn execute_anthropic(...) -> Result<ProviderResponse, ApiError>;
    async fn stream_anthropic(...) -> Result<Pin<Box<dyn Stream<...>>>, ApiError>;

    // NEW: Response normalization (providers override as needed)
    // Called after execute_openai/execute_anthropic for non-streaming responses
    fn normalize_response_for_openai_endpoint(&self, model: &str, body: &Value) -> Value {
        body.clone() // Default: response is already OpenAI format
    }
    fn normalize_response_for_anthropic_endpoint(&self, model: &str, body: &Value) -> Value {
        body.clone() // Default: response is already Anthropic format
    }
}
```

#### 1.2: Move response conversion into providers
- Move `anthropic_response_to_openai()` from routes/mod.rs → implement as `AnthropicProvider::normalize_response_for_openai_endpoint()`
- Move `openai_response_to_anthropic()` from routes/mod.rs → implement as `OpenAICodexProvider::normalize_response_for_anthropic_endpoint()` (and CopilotProvider, QwenProvider)
- Move Gemini conversion to `GeminiProvider::normalize_response_for_openai_endpoint()`

#### 1.3: Introduce ProviderMap
```rust
// providers/mod.rs
pub type ProviderMap = Arc<HashMap<ProviderId, Arc<dyn Provider>>>;

pub fn build_provider_map() -> ProviderMap {
    let mut map = HashMap::new();
    map.insert(ProviderId::Anthropic, Arc::new(AnthropicProvider::new()) as Arc<dyn Provider>);
    map.insert(ProviderId::OpenAICodex, Arc::new(OpenAICodexProvider::new()) as Arc<dyn Provider>);
    map.insert(ProviderId::Gemini, Arc::new(GeminiProvider::new()) as Arc<dyn Provider>);
    map.insert(ProviderId::Copilot, Arc::new(CopilotProvider::new()) as Arc<dyn Provider>);
    map.insert(ProviderId::Qwen, Arc::new(QwenProvider::new()) as Arc<dyn Provider>);
    Arc::new(map)
}
```

#### 1.4: Replace match chains with map lookup
```rust
// routes/mod.rs — handle_direct_openai becomes:
async fn handle_direct_openai(state: &AppState, provider_id: ProviderId, creds: ProviderCredentials, req: &ChatCompletionRequest) -> Result<Response, ApiError> {
    let provider = state.providers.get(&provider_id)
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("Provider {:?} not registered", provider_id)))?;
    let ctx = ProviderContext { credentials: &creds, model: &req.model };

    if req.stream {
        let stream = provider.stream_openai(&ctx, req).await?;
        // ... SSE response (same as now)
    } else {
        let resp = provider.execute_openai(&ctx, req).await?;
        let body = provider.normalize_response_for_openai_endpoint(&req.model, &resp.body);
        Ok(Json(body).into_response())
    }
}
```

#### 1.5: Update AppState
```rust
// Replace individual provider fields:
-    pub anthropic_provider: Arc<AnthropicProvider>,
-    pub openai_codex_provider: Arc<OpenAICodexProvider>,
-    pub gemini_provider: Arc<GeminiProvider>,
-    pub copilot_provider: Arc<CopilotProvider>,
-    pub qwen_provider: Arc<QwenProvider>,
+    pub providers: ProviderMap,
```

---

### Wave 2: Encapsulate provider-specific state
**Goal:** Move provider-specific caches and auth state into provider structs
**Risk:** Medium — internal refactoring, requires updating web_ui auth handlers
**Files:** `providers/copilot.rs`, `providers/qwen.rs`, `routes/mod.rs` (AppState), `web_ui/copilot_auth.rs`, `web_ui/qwen_auth.rs`

#### 2.1: CopilotProvider encapsulates its state
```rust
pub struct CopilotProvider {
    client: reqwest::Client,
    token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>,
    device_pending: CopilotDevicePendingMap,
}
```

#### 2.2: QwenProvider encapsulates its state
```rust
pub struct QwenProvider {
    client: reqwest::Client,
    rate_limiter: Arc<DashMap<String, VecDeque<Instant>>>,
    device_pending: QwenDevicePendingMap,
}
```

#### 2.3: Remove from AppState
```rust
-    pub copilot_token_cache: Arc<DashMap<Uuid, (String, String, Instant)>>,
-    pub copilot_device_pending: CopilotDevicePendingMap,
-    pub qwen_device_pending: QwenDevicePendingMap,
```

#### 2.4: Update web_ui auth handlers
- `copilot_auth.rs` handler functions receive CopilotProvider from ProviderMap instead of accessing AppState fields directly
- `qwen_auth.rs` same pattern

---

### Wave 3: Split routes/mod.rs into sub-modules
**Goal:** Break the 1395-line monolith into focused files
**Risk:** Low — pure file reorganization, no logic changes
**Files:** New files in `routes/`

#### 3.1: Extract `routes/state.rs`
- `AppState` struct + `impl AppState` (require_config_db, evict_user_caches)
- `UserKiroCreds`, `SessionInfo`, `OAuthPendingState`
- Re-export from `routes/mod.rs`

#### 3.2: Extract `routes/pipeline.rs`
- `extract_last_user_message` / `extract_last_user_message_anthropic`
- `extract_assistant_content` / `extract_assistant_content_anthropic`
- `build_request_context_openai` / `build_request_context_anthropic`
- `run_input_guardrail_check` / `run_output_guardrail_check`
- `inject_mcp_tools`

#### 3.3: Extract `routes/openai.rs`
- `chat_completions_handler`
- `handle_direct_openai`
- `get_models_handler`

#### 3.4: Extract `routes/anthropic.rs`
- `anthropic_messages_handler`
- `handle_direct_anthropic`

#### 3.5: Slim down `routes/mod.rs`
- Only router setup functions (`health_routes`, `openai_routes`, `anthropic_routes`)
- Re-exports of sub-modules

---

### Wave 4: Unify pipeline logic
**Goal:** Reduce ~400 lines of duplication between handlers
**Risk:** Medium-High — touching handler core logic
**Files:** `routes/pipeline.rs`, `routes/openai.rs`, `routes/anthropic.rs`

Extract common stages into a `PipelineContext`:
```rust
pub struct PipelineContext {
    pub user_creds: Option<UserKiroCreds>,
    pub headers: HeaderMap,
    pub config: Config,
    pub conversation_id: String,
    pub profile_arn: String,
}

pub async fn prepare_pipeline(state: &AppState, raw_request: &Request<Body>) -> Result<PipelineContext, ApiError>;
```

Both handlers call `prepare_pipeline()` then diverge only for format-specific conversion and streaming.

---

### Wave 5: Complete KiroProvider
**Goal:** Make Kiro a real provider, remove inlined pipeline from handlers
**Risk:** High — replaces the core request path
**Files:** `providers/kiro.rs`, `routes/openai.rs`, `routes/anthropic.rs`

Wire KiroProvider with:
- Converter functions (build_kiro_payload)
- HTTP client + auth token resolution
- AWS Event Stream parser
- Streaming format conversion (kiro_to_openai/kiro_to_anthropic)

This makes all requests flow through the Provider trait uniformly.

---

## What changes per wave

| File | Wave 1 | Wave 2 | Wave 3 | Wave 4 | Wave 5 |
|------|--------|--------|--------|--------|--------|
| `providers/traits.rs` | Add normalize methods | — | — | — | — |
| `providers/mod.rs` | Add ProviderMap, build fn | — | — | — | — |
| `providers/anthropic.rs` | Add normalize impl | — | — | — | — |
| `providers/gemini.rs` | Add normalize impl | — | — | — | — |
| `providers/openai_codex.rs` | Add normalize impl | — | — | — | — |
| `providers/copilot.rs` | Add normalize impl | Encapsulate state | — | — | — |
| `providers/qwen.rs` | Add normalize impl | Encapsulate state | — | — | — |
| `providers/kiro.rs` | — | — | — | — | Full impl |
| `routes/mod.rs` | Replace match chains, update AppState | Remove provider-specific fields | Split into sub-modules | — | — |
| `routes/state.rs` | — | — | Created | — | — |
| `routes/pipeline.rs` | — | — | Created | Extract shared logic | — |
| `routes/openai.rs` | — | — | Created | Use PipelineContext | Remove inlined Kiro |
| `routes/anthropic.rs` | — | — | Created | Use PipelineContext | Remove inlined Kiro |
| `main.rs` | Use build_provider_map() | Pass provider refs to auth handlers | — | — | — |
| `web_ui/copilot_auth.rs` | — | Get provider from map | — | — | — |
| `web_ui/qwen_auth.rs` | — | Get provider from map | — | — | — |

## Verification

After each wave:
1. `cd backend && cargo clippy --all-targets` — zero warnings
2. `cd backend && cargo test --lib` — all 395+ tests pass
3. `cd backend && cargo fmt --check` — no diffs

Full E2E verification after Wave 1+2:
- Test direct provider routing (send request with `anthropic/claude-*` model)
- Test Kiro fallback routing (send request without provider prefix)
- Test streaming and non-streaming for each provider

## Decisions Made

1. **Scope:** All 5 waves in one pass — full modularization including pipeline dedup and KiroProvider completion
2. **Auth handler access:** Downcast from ProviderMap — `state.providers.get(&ProviderId::Copilot)` then `downcast_ref::<CopilotProvider>()`
3. **ProviderMap type:** `HashMap<ProviderId, Arc<dyn Provider>>` wrapped in `Arc` — providers are immutable after startup, no need for DashMap overhead
