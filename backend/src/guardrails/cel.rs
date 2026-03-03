use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::Result;
use cel_interpreter::{objects::Key, Program, Value};

use super::types::RequestContext;

/// CEL expression evaluator with compiled program cache.
pub struct CelEvaluator {
    cache: RwLock<HashMap<String, Program>>,
}

impl Default for CelEvaluator {
    fn default() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl CelEvaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate and compile a CEL expression, caching the result.
    ///
    /// Returns `Ok(())` if the expression is valid, `Err` with details if not.
    pub fn compile(&self, expression: &str) -> Result<()> {
        if expression.trim().is_empty() {
            return Ok(());
        }

        let program = Program::compile(expression)
            .map_err(|e| anyhow::anyhow!("CEL compile error: {:?}", e))?;

        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        cache.insert(expression.to_string(), program);
        Ok(())
    }

    /// Evaluate a CEL expression against a RequestContext.
    ///
    /// Empty expressions always return `true` (match all requests).
    /// Returns `true` if the expression matches, `false` otherwise.
    pub fn evaluate(&self, expression: &str, ctx: &RequestContext) -> Result<bool> {
        if expression.trim().is_empty() {
            return Ok(true);
        }

        // Try cache first
        {
            let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
            if let Some(program) = cache.get(expression) {
                let cel_ctx = build_cel_context(ctx);
                return execute_program(program, &cel_ctx);
            }
        }

        // Not cached — compile, cache, and evaluate
        let program = Program::compile(expression)
            .map_err(|e| anyhow::anyhow!("CEL compile error: {:?}", e))?;

        let cel_ctx = build_cel_context(ctx);
        let result = execute_program(&program, &cel_ctx);

        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        cache.insert(expression.to_string(), program);

        result
    }

    /// Clear the compiled program cache (call after rules are updated).
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
        cache.clear();
    }

    /// Validate a CEL expression without caching.
    ///
    /// Returns `Ok(())` if valid, or `Err` with the parse/compile error.
    pub fn validate(expression: &str) -> Result<()> {
        if expression.trim().is_empty() {
            return Ok(());
        }
        Program::compile(expression)
            .map_err(|e| anyhow::anyhow!("CEL compile error: {:?}", e))?;
        Ok(())
    }
}

/// Build the CEL evaluation context from a RequestContext.
///
/// Maps fields to a `request` object:
/// - `request.model` (string)
/// - `request.api_format` (string)
/// - `request.message_count` (int)
/// - `request.has_tools` (bool)
/// - `request.is_streaming` (bool)
/// - `request.content_length` (int)
fn build_cel_context(ctx: &RequestContext) -> cel_interpreter::Context<'static> {
    let mut request_map: HashMap<Key, Value> = HashMap::new();
    request_map.insert(
        Key::from("model"),
        Value::String(ctx.model.clone().into()),
    );
    request_map.insert(
        Key::from("api_format"),
        Value::String(ctx.api_format.clone().into()),
    );
    request_map.insert(
        Key::from("message_count"),
        Value::Int(ctx.message_count as i64),
    );
    request_map.insert(Key::from("has_tools"), Value::Bool(ctx.has_tools));
    request_map.insert(Key::from("is_streaming"), Value::Bool(ctx.is_streaming));
    request_map.insert(
        Key::from("content_length"),
        Value::Int(ctx.content_length as i64),
    );

    let mut cel_ctx = cel_interpreter::Context::default();
    cel_ctx
        .add_variable("request", Value::Map(request_map.into()))
        .unwrap();
    cel_ctx
}

/// Execute a compiled CEL program and extract a boolean result.
fn execute_program(program: &Program, ctx: &cel_interpreter::Context) -> Result<bool> {
    let result = program
        .execute(ctx)
        .map_err(|e| anyhow::anyhow!("CEL execution error: {:?}", e))?;

    match result {
        Value::Bool(b) => Ok(b),
        other => anyhow::bail!("CEL expression must return bool, got: {:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> RequestContext {
        RequestContext {
            model: "claude-sonnet-4-20250514".to_string(),
            api_format: "openai".to_string(),
            message_count: 5,
            has_tools: true,
            is_streaming: false,
            content_length: 1500,
        }
    }

    #[test]
    fn test_empty_expression_always_true() {
        let evaluator = CelEvaluator::new();
        let ctx = test_context();
        assert!(evaluator.evaluate("", &ctx).unwrap());
        assert!(evaluator.evaluate("  ", &ctx).unwrap());
    }

    #[test]
    fn test_model_match() {
        let evaluator = CelEvaluator::new();
        let ctx = test_context();

        assert!(evaluator
            .evaluate("request.model == \"claude-sonnet-4-20250514\"", &ctx)
            .unwrap());
        assert!(!evaluator
            .evaluate("request.model == \"gpt-4\"", &ctx)
            .unwrap());
    }

    #[test]
    fn test_api_format_match() {
        let evaluator = CelEvaluator::new();
        let ctx = test_context();

        assert!(evaluator
            .evaluate("request.api_format == \"openai\"", &ctx)
            .unwrap());
        assert!(!evaluator
            .evaluate("request.api_format == \"anthropic\"", &ctx)
            .unwrap());
    }

    #[test]
    fn test_numeric_comparison() {
        let evaluator = CelEvaluator::new();
        let ctx = test_context();

        assert!(evaluator
            .evaluate("request.message_count > 3", &ctx)
            .unwrap());
        assert!(!evaluator
            .evaluate("request.message_count > 10", &ctx)
            .unwrap());
        assert!(evaluator
            .evaluate("request.content_length >= 1500", &ctx)
            .unwrap());
    }

    #[test]
    fn test_boolean_fields() {
        let evaluator = CelEvaluator::new();
        let ctx = test_context();

        assert!(evaluator.evaluate("request.has_tools", &ctx).unwrap());
        assert!(!evaluator.evaluate("request.is_streaming", &ctx).unwrap());
        assert!(evaluator.evaluate("!request.is_streaming", &ctx).unwrap());
    }

    #[test]
    fn test_compound_expression() {
        let evaluator = CelEvaluator::new();
        let ctx = test_context();

        assert!(evaluator
            .evaluate(
                "request.api_format == \"openai\" && request.message_count > 2",
                &ctx
            )
            .unwrap());
        assert!(!evaluator
            .evaluate(
                "request.api_format == \"anthropic\" || request.message_count > 10",
                &ctx
            )
            .unwrap());
    }

    #[test]
    fn test_invalid_expression() {
        let result = CelEvaluator::validate("this is not valid CEL !!!@#$");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_valid_expression() {
        let result = CelEvaluator::validate("request.model == \"test\"");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_empty_expression() {
        let result = CelEvaluator::validate("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_and_cache() {
        let evaluator = CelEvaluator::new();
        assert!(evaluator
            .compile("request.model == \"test\"")
            .is_ok());

        // Second call should use cache
        let ctx = test_context();
        assert!(!evaluator
            .evaluate("request.model == \"test\"", &ctx)
            .unwrap());
    }

    #[test]
    fn test_clear_cache() {
        let evaluator = CelEvaluator::new();
        evaluator.compile("request.model == \"test\"").unwrap();
        evaluator.clear_cache();

        // After clear, the program must be re-compiled
        let ctx = test_context();
        assert!(!evaluator
            .evaluate("request.model == \"test\"", &ctx)
            .unwrap());
    }
}
