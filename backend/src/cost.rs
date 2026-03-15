use std::collections::HashMap;
use std::sync::LazyLock;

/// Static pricing data: model name -> (input_cost_per_token, output_cost_per_token)
/// Costs are in USD per token (e.g., 3e-6 = $3 per million tokens)
static PRICING_DATA: LazyLock<HashMap<&'static str, (f64, f64)>> = LazyLock::new(|| {
    let mut map = HashMap::new();

    // Claude models
    map.insert("claude-sonnet-4-5-20250514", (3e-6, 15e-6));
    map.insert("claude-haiku-4-5-20251001", (1e-6, 5e-6));
    map.insert("claude-opus-4-6-20250610", (15e-6, 75e-6));

    // Amazon Nova models
    map.insert("amazon.nova-pro-v1:0", (0.8e-6, 3.2e-6));
    map.insert("amazon.nova-lite-v1:0", (0.06e-6, 0.24e-6));
    map.insert("amazon.nova-micro-v1:0", (0.035e-6, 0.14e-6));

    // OpenAI models
    map.insert("gpt-4o", (2.5e-6, 10e-6));
    map.insert("gpt-4o-mini", (0.15e-6, 0.6e-6));

    map
});

/// Calculate the cost for a request based on model and token usage.
///
/// # Arguments
/// * `model_id` - The model identifier (e.g., "claude-sonnet-4-5-20250514")
/// * `input_tokens` - Number of input/prompt tokens
/// * `output_tokens` - Number of output/completion tokens
///
/// # Returns
/// Total cost in USD. Returns 0.0 for unknown models.
pub fn calculate_cost(model_id: &str, input_tokens: i64, output_tokens: i64) -> f64 {
    // Try exact match first
    if let Some((input_cost, output_cost)) = PRICING_DATA.get(model_id) {
        return (input_tokens as f64) * input_cost + (output_tokens as f64) * output_cost;
    }

    // Try prefix matching (e.g., "claude-sonnet-4-5" matches "claude-sonnet-4-5-20250514")
    for (key, (input_cost, output_cost)) in PRICING_DATA.iter() {
        // Check if model_id starts with the key or key starts with model_id
        if model_id.starts_with(*key) || key.starts_with(model_id) {
            return (input_tokens as f64) * input_cost + (output_tokens as f64) * output_cost;
        }
    }

    // Unknown model - return 0.0
    tracing::debug!(model_id = %model_id, "Unknown model for cost calculation");
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_cost_exact_match() {
        // Claude Sonnet 4.5: $3/M input, $15/M output
        let cost = calculate_cost("claude-sonnet-4-5-20250514", 1_000_000, 500_000);
        assert!((cost - 10.5).abs() < 0.001, "Expected ~10.5, got {}", cost);
        // 1M * 3e-6 + 0.5M * 15e-6 = 3 + 7.5 = 10.5
    }

    #[test]
    fn test_calculate_cost_claude_haiku() {
        // Claude Haiku 4.5: $1/M input, $5/M output
        let cost = calculate_cost("claude-haiku-4-5-20251001", 2_000_000, 1_000_000);
        assert!((cost - 7.0).abs() < 0.001, "Expected ~7.0, got {}", cost);
        // 2M * 1e-6 + 1M * 5e-6 = 2 + 5 = 7
    }

    #[test]
    fn test_calculate_cost_claude_opus() {
        // Claude Opus 4.6: $15/M input, $75/M output
        let cost = calculate_cost("claude-opus-4-6-20250610", 100_000, 50_000);
        assert!((cost - 5.25).abs() < 0.001, "Expected ~5.25, got {}", cost);
        // 0.1M * 15e-6 + 0.05M * 75e-6 = 1.5 + 3.75 = 5.25
    }

    #[test]
    fn test_calculate_cost_amazon_nova() {
        // Amazon Nova Pro: $0.8/M input, $3.2/M output
        let cost = calculate_cost("amazon.nova-pro-v1:0", 1_000_000, 1_000_000);
        assert!((cost - 4.0).abs() < 0.001, "Expected ~4.0, got {}", cost);
        // 1M * 0.8e-6 + 1M * 3.2e-6 = 0.8 + 3.2 = 4.0
    }

    #[test]
    fn test_calculate_cost_openai_gpt4o() {
        // GPT-4o: $2.5/M input, $10/M output
        let cost = calculate_cost("gpt-4o", 1_000_000, 500_000);
        assert!((cost - 7.5).abs() < 0.001, "Expected ~7.5, got {}", cost);
        // 1M * 2.5e-6 + 0.5M * 10e-6 = 2.5 + 5 = 7.5
    }

    #[test]
    fn test_calculate_cost_openai_gpt4o_mini() {
        // GPT-4o-mini: $0.15/M input, $0.6/M output
        let cost = calculate_cost("gpt-4o-mini", 10_000_000, 5_000_000);
        assert!((cost - 4.5).abs() < 0.001, "Expected ~4.5, got {}", cost);
        // 10M * 0.15e-6 + 5M * 0.6e-6 = 1.5 + 3 = 4.5
    }

    #[test]
    fn test_calculate_cost_prefix_match() {
        // Test prefix matching - shorter model_id should match longer key
        let cost = calculate_cost("claude-sonnet-4-5", 1_000_000, 0);
        assert!((cost - 3.0).abs() < 0.001, "Expected ~3.0, got {}", cost);
        // Should match claude-sonnet-4-5-20250514
    }

    #[test]
    fn test_calculate_cost_unknown_model() {
        // Unknown model should return 0.0
        let cost = calculate_cost("unknown-model-v1", 1_000_000, 500_000);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_calculate_cost_zero_tokens() {
        let cost = calculate_cost("gpt-4o", 0, 0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_calculate_cost_negative_tokens() {
        // Negative tokens should still calculate (though this shouldn't happen in practice)
        let cost = calculate_cost("gpt-4o", -1_000_000, -500_000);
        assert!(
            (cost - (-7.5)).abs() < 0.001,
            "Expected ~-7.5, got {}",
            cost
        );
    }
}
