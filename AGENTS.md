# AGENTS.md

Guidelines for AI coding agents working in this Rust proxy gateway codebase.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build (optimized)
cargo run --bin kiro-gateway --release      # Build and run the server
cargo run --bin probe_limits --release -- --model <model-id>  # Probe model limits
cargo run --bin probe_limits --release -- --all-models        # Probe all models
cargo clippy             # Lint - fix all warnings before committing
cargo fmt                # Format code (run before committing)
cargo fmt -- --check     # Check formatting without modifying
```

## Testing

```bash
cargo test --lib                    # Run all unit tests (recommended)
cargo test --lib <test_name>        # Run a specific test by name
cargo test --lib <module>::         # Run all tests in a module
cargo test --lib -- --nocapture     # Show println! output during tests
```

Examples:
```bash
cargo test --lib test_convert_openai_messages_basic
cargo test --lib converters::openai_to_kiro::tests::
cargo test --lib auth::
```

## Required Environment Variables

Set in `.env` or export:
- `PROXY_API_KEY` - Password to protect the proxy server (required)
- `KIRO_CLI_DB_FILE` - Path to kiro-cli SQLite database, e.g. `~/.kiro/data.db` (required)
- `KIRO_REGION` - AWS region (default: `us-east-1`)

## Architecture Overview

```
Client Request (OpenAI/Anthropic format)
    ↓
routes/mod.rs (Axum HTTP handlers)
    ↓
converters/ (format translation)
    ├── openai_to_kiro.rs
    └── anthropic_to_kiro.rs
    ↓
http_client.rs → Kiro API
    ↓
streaming/mod.rs (AWS Event Stream parsing)
    ↓
converters/
    ├── kiro_to_openai.rs
    └── kiro_to_anthropic.rs
    ↓
Client Response (SSE stream)
```

### Key Modules
- `routes/` - Axum handlers for `/v1/chat/completions` (OpenAI) and `/v1/messages` (Anthropic)
- `converters/` - Bidirectional format conversion
- `streaming/` - AWS Event Stream → SSE conversion
- `auth/` - Token management with auto-refresh from SQLite
- `models/` - Request/response type definitions
- `error.rs` - Centralized error types

## Code Style Guidelines

### Imports
Order imports in groups separated by blank lines:
1. `std` library
2. External crates (alphabetically)
3. Internal crate modules (`crate::`)

```rust
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::ApiError;
```

### Naming Conventions
- Types/Structs/Enums: `PascalCase`
- Functions/Methods/Variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Module files: `snake_case.rs`

### Struct Definitions
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyStruct {
    pub required_field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_field: Option<String>,
}
```

### Builder Pattern
Use `with_*` methods for optional configuration:
```rust
impl MyStruct {
    pub fn new(required: String) -> Self {
        Self { required_field: required, optional_field: None }
    }

    pub fn with_optional(mut self, value: String) -> Self {
        self.optional_field = Some(value);
        self
    }
}
```

### Error Handling
- Use `thiserror` for defining error enums
- Use `anyhow::Result` with `.context()` for propagating errors
- Implement `IntoResponse` for API errors

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Authentication failed: {0}")]
    AuthError(String),
    #[error("Invalid request: {0}")]
    BadRequest(String),
}

// In functions:
let data = fetch_data().context("Failed to fetch data")?;
```

### Async and Concurrency
- Use `tokio` for async runtime
- Use `Arc<RwLock<T>>` for shared mutable state
- Prefer `tokio::sync` primitives over `std::sync` in async code

### Logging
Use `tracing` macros with structured fields:
```rust
use tracing::{debug, info, warn, error};

debug!(model = %model_id, "Processing request");
info!(tokens = count, "Request completed");
error!(error = ?err, "Failed to process");
```

### Documentation
- Use `///` doc comments for public items
- Include examples for complex functions
- Document panics and errors

```rust
/// Converts an OpenAI request to Kiro format.
///
/// # Errors
/// Returns `ApiError::BadRequest` if the model is not supported.
pub fn convert_request(req: OpenAiRequest) -> Result<KiroRequest, ApiError> {
    // ...
}
```

### Testing
- Place unit tests in `#[cfg(test)] mod tests` at bottom of file
- Use descriptive test names: `test_<function>_<scenario>`
- Create helper functions for test setup (e.g., `create_test_config()`)
- Use `#[tokio::test]` for async tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> Config {
        Config::default()
    }

    #[test]
    fn test_convert_messages_basic() {
        let config = create_test_config();
        // ...
    }

    #[tokio::test]
    async fn test_async_operation() {
        // ...
    }
}
```

### Conditional Compilation
Use feature flags for test utilities:
```rust
#[cfg(any(test, feature = "test-utils"))]
pub fn mock_function() { }
```

### Code Organization
- Keep modules focused and single-purpose
- Re-export public items in `mod.rs`
- Use section comments for large files: `// === Section Name ===`
- Mark unused code with `#[allow(dead_code)]` if intentionally kept

## Common Patterns

### HTTP Response Streaming
The codebase uses Axum's streaming responses with `futures::stream`:
```rust
use futures::stream::Stream;
use axum::response::sse::{Event, Sse};
```

### Configuration
Access config via dependency injection, not global state.

### Model Resolution
Use `resolver.rs` for model name normalization - don't hardcode model IDs.
