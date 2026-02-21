//! Anthropic-to-Google request converter for Cloud Code API.
//!
//! Converts Anthropic Messages API requests into the Google Generative AI
//! format expected by the Cloud Code `generateContent` endpoint.

use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::antigravity::constants::{
    get_model_family, is_thinking_model, ModelFamily, GEMINI_MAX_OUTPUT_TOKENS,
};
use crate::models::anthropic::AnthropicMessagesRequest;

use super::content_converter::{clean_cache_control, convert_content_to_parts, convert_role};
use super::schema_sanitizer::sanitize_schema;

/// Converts an Anthropic Messages API request to Google Generative AI format.
///
/// This is the main entry point for the Anthropic → Google conversion pipeline.
///
/// # Conversion steps
///
/// 1. Strip `cache_control` fields from all messages
/// 2. System prompt → `systemInstruction.parts[{text}]`
/// 3. Messages: user→user, assistant→model
/// 4. Content blocks → Google parts (text, inlineData, functionCall, functionResponse, thinking)
/// 5. Tools → `[{functionDeclarations: [{name, description, parameters}]}]`
/// 6. Sanitize tool schemas for Google protobuf compatibility
/// 7. Build `generationConfig` (maxOutputTokens, temperature, topP, topK, stopSequences)
/// 8. Thinking config: Claude uses snake_case, Gemini uses camelCase
/// 9. Cap Gemini max tokens to 16384
/// 10. Empty parts get placeholder text "."
pub fn convert_anthropic_to_google(request: &AnthropicMessagesRequest) -> Value {
    let model_name = &request.model;
    let family = get_model_family(model_name);
    let is_claude = family == ModelFamily::Claude;
    let is_gemini = family == ModelFamily::Gemini;
    let is_thinking = is_thinking_model(model_name);

    let mut google_request = json!({
        "contents": [],
        "generationConfig": {}
    });

    // 1. System instruction
    build_system_instruction(
        &mut google_request,
        &request.system,
        is_claude,
        is_thinking,
        request.tools.as_ref(),
    );

    // 2. Convert messages (after stripping cache_control)
    let raw_messages: Vec<Value> = request
        .messages
        .iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();
    let cleaned = clean_cache_control(&raw_messages);

    let contents = google_request["contents"].as_array_mut().unwrap();
    for msg in &cleaned {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").unwrap_or(&Value::Null);

        let mut parts = convert_content_to_parts(content, is_claude, is_gemini);

        // Empty parts get placeholder text
        if parts.is_empty() {
            warn!("Empty parts array after filtering, adding placeholder");
            parts.push(json!({ "text": "." }));
        }

        contents.push(json!({
            "role": convert_role(role),
            "parts": parts
        }));
    }

    // 3. Generation config
    build_generation_config(
        &mut google_request,
        request,
        is_claude,
        is_gemini,
        is_thinking,
    );

    // 4. Tools
    build_tools(&mut google_request, request.tools.as_ref(), is_claude);

    debug!(
        model = %model_name,
        messages = cleaned.len(),
        tools = request.tools.as_ref().map_or(0, |t| t.len()),
        "Converted Anthropic request to Google format"
    );

    google_request
}

/// Builds the `systemInstruction` field from the Anthropic system prompt.
fn build_system_instruction(
    request: &mut Value,
    system: &Option<Value>,
    is_claude: bool,
    is_thinking: bool,
    tools: Option<&Vec<crate::models::anthropic::AnthropicTool>>,
) {
    let mut system_parts: Vec<Value> = Vec::new();

    if let Some(system) = system {
        if let Some(text) = system.as_str() {
            system_parts.push(json!({ "text": text }));
        } else if let Some(blocks) = system.as_array() {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        system_parts.push(json!({ "text": text }));
                    }
                }
            }
        }
    }

    // Add interleaved thinking hint for Claude thinking models with tools
    if is_claude && is_thinking && tools.is_some_and(|t| !t.is_empty()) {
        let hint = "Interleaved thinking is enabled. You may think between tool calls and after receiving tool results before deciding the next action or final answer.";
        if let Some(last) = system_parts.last_mut() {
            if let Some(text) = last.get("text").and_then(|v| v.as_str()) {
                *last = json!({ "text": format!("{text}\n\n{hint}") });
            } else {
                system_parts.push(json!({ "text": hint }));
            }
        } else {
            system_parts.push(json!({ "text": hint }));
        }
    }

    if !system_parts.is_empty() {
        request["systemInstruction"] = json!({ "parts": system_parts });
    }
}

/// Builds the `generationConfig` field including thinking config.
fn build_generation_config(
    request: &mut Value,
    anthropic_req: &AnthropicMessagesRequest,
    is_claude: bool,
    is_gemini: bool,
    is_thinking: bool,
) {
    let config = request["generationConfig"].as_object_mut().unwrap();

    // maxOutputTokens
    config.insert(
        "maxOutputTokens".to_string(),
        json!(anthropic_req.max_tokens),
    );

    // Sampling parameters
    if let Some(temp) = anthropic_req.temperature {
        config.insert("temperature".to_string(), json!(temp));
    }
    if let Some(top_p) = anthropic_req.top_p {
        config.insert("topP".to_string(), json!(top_p));
    }
    if let Some(top_k) = anthropic_req.top_k {
        config.insert("topK".to_string(), json!(top_k));
    }
    if let Some(ref stop) = anthropic_req.stop_sequences {
        if !stop.is_empty() {
            config.insert("stopSequences".to_string(), json!(stop));
        }
    }

    // Thinking config
    if is_thinking {
        let thinking_budget = anthropic_req
            .metadata
            .as_ref()
            .and_then(|m| m.get("thinking"))
            .and_then(|t| t.get("budget_tokens"))
            .and_then(|b| b.as_i64());

        if is_claude {
            let mut thinking_config = json!({ "include_thoughts": true });
            if let Some(budget) = thinking_budget {
                thinking_config["thinking_budget"] = json!(budget);
                debug!(budget, "Claude thinking enabled with budget");

                // Validate max_tokens > thinking_budget
                let max_tokens = anthropic_req.max_tokens as i64;
                if max_tokens <= budget {
                    let adjusted = budget + 8192;
                    warn!(
                        max_tokens,
                        budget, adjusted, "max_tokens <= thinking_budget, adjusting"
                    );
                    config.insert("maxOutputTokens".to_string(), json!(adjusted));
                }
            }
            config.insert("thinkingConfig".to_string(), thinking_config);
        } else if is_gemini {
            let budget = thinking_budget.unwrap_or(16000);
            let thinking_config = json!({
                "includeThoughts": true,
                "thinkingBudget": budget
            });
            debug!(budget, "Gemini thinking enabled with budget");
            config.insert("thinkingConfig".to_string(), thinking_config);
        }
    }

    // Cap Gemini max tokens
    if is_gemini {
        if let Some(max) = config.get("maxOutputTokens").and_then(|v| v.as_u64()) {
            if max > GEMINI_MAX_OUTPUT_TOKENS as u64 {
                debug!(
                    from = max,
                    to = GEMINI_MAX_OUTPUT_TOKENS,
                    "Capping Gemini max_tokens"
                );
                config.insert(
                    "maxOutputTokens".to_string(),
                    json!(GEMINI_MAX_OUTPUT_TOKENS),
                );
            }
        }
    }
}

/// Builds the `tools` and optional `toolConfig` fields.
fn build_tools(
    request: &mut Value,
    tools: Option<&Vec<crate::models::anthropic::AnthropicTool>>,
    is_claude: bool,
) {
    let Some(tools) = tools else { return };
    if tools.is_empty() {
        return;
    }

    let declarations: Vec<Value> = tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            // Sanitize name: only alphanumeric, underscore, hyphen; max 64 chars
            let name = if tool.name.is_empty() {
                format!("tool-{idx}")
            } else {
                tool.name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '_' || c == '-' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .take(64)
                    .collect()
            };

            let description = tool.description.as_deref().unwrap_or("");
            let parameters = sanitize_schema(&tool.input_schema);

            json!({
                "name": name,
                "description": description,
                "parameters": parameters
            })
        })
        .collect();

    request["tools"] = json!([{ "functionDeclarations": declarations }]);

    // Claude models use VALIDATED function calling mode
    if is_claude {
        request["toolConfig"] = json!({
            "functionCallingConfig": {
                "mode": "VALIDATED"
            }
        });
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{AnthropicMessage, AnthropicMessagesRequest, AnthropicTool};
    use serde_json::json;

    fn make_request(model: &str, messages: Vec<AnthropicMessage>) -> AnthropicMessagesRequest {
        AnthropicMessagesRequest {
            model: model.to_string(),
            messages,
            max_tokens: 4096,
            system: None,
            stream: false,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
        }
    }

    fn msg(role: &str, content: Value) -> AnthropicMessage {
        AnthropicMessage {
            role: role.to_string(),
            content,
        }
    }

    // === Basic conversion ===

    #[test]
    fn test_basic_text_message() {
        let req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hello"))]);
        let result = convert_anthropic_to_google(&req);

        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_role_mapping() {
        let req = make_request(
            "claude-sonnet-4-5",
            vec![msg("user", json!("Hi")), msg("assistant", json!("Hello"))],
        );
        let result = convert_anthropic_to_google(&req);
        let contents = result["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }

    // === System instruction ===

    #[test]
    fn test_system_string() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.system = Some(json!("You are helpful."));
        let result = convert_anthropic_to_google(&req);

        assert_eq!(
            result["systemInstruction"]["parts"][0]["text"],
            "You are helpful."
        );
    }

    #[test]
    fn test_system_blocks() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.system = Some(json!([
            {"type": "text", "text": "Be helpful."},
            {"type": "text", "text": "Be concise."}
        ]));
        let result = convert_anthropic_to_google(&req);

        let parts = result["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "Be helpful.");
        assert_eq!(parts[1]["text"], "Be concise.");
    }

    #[test]
    fn test_system_blocks_with_cache_control_ignored() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.system = Some(json!([
            {"type": "text", "text": "System prompt", "cache_control": {"type": "ephemeral"}}
        ]));
        let result = convert_anthropic_to_google(&req);
        assert_eq!(
            result["systemInstruction"]["parts"][0]["text"],
            "System prompt"
        );
    }

    #[test]
    fn test_no_system_instruction_when_none() {
        let req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        let result = convert_anthropic_to_google(&req);
        assert!(result.get("systemInstruction").is_none());
    }

    // === Generation config ===

    #[test]
    fn test_generation_config_basic() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.top_k = Some(40);
        req.stop_sequences = Some(vec!["STOP".to_string()]);

        let result = convert_anthropic_to_google(&req);
        let config = &result["generationConfig"];

        assert_eq!(config["maxOutputTokens"], 4096);
        let temp = config["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001, "temperature was {temp}");
        let top_p = config["topP"].as_f64().unwrap();
        assert!((top_p - 0.9).abs() < 0.001, "topP was {top_p}");
        assert_eq!(config["topK"], 40);
        assert_eq!(config["stopSequences"][0], "STOP");
    }

    // === Thinking config ===

    #[test]
    fn test_claude_thinking_config() {
        let req = make_request("claude-sonnet-4-5-thinking", vec![msg("user", json!("Hi"))]);
        let result = convert_anthropic_to_google(&req);
        let thinking = &result["generationConfig"]["thinkingConfig"];

        assert_eq!(thinking["include_thoughts"], true);
        // No budget specified → no thinking_budget field
        assert!(thinking.get("thinking_budget").is_none());
    }

    #[test]
    fn test_gemini_thinking_config() {
        let req = make_request("gemini-3-flash", vec![msg("user", json!("Hi"))]);
        let result = convert_anthropic_to_google(&req);
        let thinking = &result["generationConfig"]["thinkingConfig"];

        assert_eq!(thinking["includeThoughts"], true);
        assert_eq!(thinking["thinkingBudget"], 16000);
    }

    #[test]
    fn test_non_thinking_model_no_thinking_config() {
        let req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        let result = convert_anthropic_to_google(&req);
        assert!(result["generationConfig"].get("thinkingConfig").is_none());
    }

    // === Gemini max token cap ===

    #[test]
    fn test_gemini_max_tokens_capped() {
        let mut req = make_request("gemini-3-flash", vec![msg("user", json!("Hi"))]);
        req.max_tokens = 100_000;
        let result = convert_anthropic_to_google(&req);
        assert_eq!(
            result["generationConfig"]["maxOutputTokens"],
            GEMINI_MAX_OUTPUT_TOKENS
        );
    }

    #[test]
    fn test_claude_max_tokens_not_capped() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.max_tokens = 100_000;
        let result = convert_anthropic_to_google(&req);
        assert_eq!(result["generationConfig"]["maxOutputTokens"], 100_000);
    }

    // === Tools ===

    #[test]
    fn test_tools_conversion() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.tools = Some(vec![AnthropicTool {
            name: "get_weather".to_string(),
            description: Some("Get weather info".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        }]);

        let result = convert_anthropic_to_google(&req);
        let decls = &result["tools"][0]["functionDeclarations"];
        assert_eq!(decls[0]["name"], "get_weather");
        assert_eq!(decls[0]["description"], "Get weather info");
        assert_eq!(decls[0]["parameters"]["type"], "object");
    }

    #[test]
    fn test_tools_schema_sanitized() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.tools = Some(vec![AnthropicTool {
            name: "my_tool".to_string(),
            description: None,
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "title": "MyTool",
                "properties": {
                    "x": {"type": "string", "default": "foo"}
                }
            }),
        }]);

        let result = convert_anthropic_to_google(&req);
        let params = &result["tools"][0]["functionDeclarations"][0]["parameters"];
        // Unsupported fields should be stripped
        assert!(params.get("additionalProperties").is_none());
        assert!(params.get("title").is_none());
        assert!(params["properties"]["x"].get("default").is_none());
    }

    #[test]
    fn test_claude_tool_config_validated() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.tools = Some(vec![AnthropicTool {
            name: "tool".to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
        }]);

        let result = convert_anthropic_to_google(&req);
        assert_eq!(
            result["toolConfig"]["functionCallingConfig"]["mode"],
            "VALIDATED"
        );
    }

    #[test]
    fn test_gemini_no_tool_config() {
        let mut req = make_request("gemini-3-flash", vec![msg("user", json!("Hi"))]);
        req.tools = Some(vec![AnthropicTool {
            name: "tool".to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
        }]);

        let result = convert_anthropic_to_google(&req);
        assert!(result.get("toolConfig").is_none());
    }

    #[test]
    fn test_no_tools_field_when_none() {
        let req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        let result = convert_anthropic_to_google(&req);
        assert!(result.get("tools").is_none());
    }

    // === Cache control stripping ===

    #[test]
    fn test_cache_control_stripped_from_messages() {
        let req = make_request(
            "claude-sonnet-4-5",
            vec![msg(
                "user",
                json!([
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
                ]),
            )],
        );
        let result = convert_anthropic_to_google(&req);
        let parts = result["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "Hello");
        // cache_control should not appear in the output
        assert!(parts[0].get("cache_control").is_none());
    }

    // === Empty parts placeholder ===

    #[test]
    fn test_empty_parts_get_placeholder() {
        // All content blocks are empty text → should get placeholder
        let req = make_request(
            "claude-sonnet-4-5",
            vec![msg("user", json!([{"type": "text", "text": ""}]))],
        );
        let result = convert_anthropic_to_google(&req);
        let parts = result["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], ".");
    }

    // === Interleaved thinking hint ===

    #[test]
    fn test_claude_thinking_with_tools_gets_hint() {
        let mut req = make_request("claude-sonnet-4-5-thinking", vec![msg("user", json!("Hi"))]);
        req.system = Some(json!("Be helpful."));
        req.tools = Some(vec![AnthropicTool {
            name: "tool".to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
        }]);

        let result = convert_anthropic_to_google(&req);
        let text = result["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("Interleaved thinking is enabled"));
        assert!(text.starts_with("Be helpful."));
    }

    #[test]
    fn test_gemini_thinking_no_hint() {
        let mut req = make_request("gemini-3-flash", vec![msg("user", json!("Hi"))]);
        req.system = Some(json!("Be helpful."));
        req.tools = Some(vec![AnthropicTool {
            name: "tool".to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
        }]);

        let result = convert_anthropic_to_google(&req);
        let text = result["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert!(!text.contains("Interleaved thinking"));
    }

    // === Tool name sanitization ===

    #[test]
    fn test_tool_name_sanitized() {
        let mut req = make_request("claude-sonnet-4-5", vec![msg("user", json!("Hi"))]);
        req.tools = Some(vec![AnthropicTool {
            name: "my.tool/v2!".to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
        }]);

        let result = convert_anthropic_to_google(&req);
        let name = result["tools"][0]["functionDeclarations"][0]["name"]
            .as_str()
            .unwrap();
        assert_eq!(name, "my_tool_v2_");
    }
}
