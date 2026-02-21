//! Cloud Code SSE streaming parser.
//!
//! Parses Server-Sent Events from the Cloud Code `streamGenerateContent` endpoint
//! and converts them to Anthropic Messages API streaming events or a collected response.

use std::collections::HashMap;

use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::antigravity::constants::{get_model_family, ModelFamily};

/// Minimum length for a valid thinking signature.
const MIN_SIGNATURE_LENGTH: usize = 50;

// === SSE Line Parser ===

/// Extracts JSON data from SSE lines.
///
/// Parses `data: {...}` lines from the SSE stream, skipping empty lines
/// and non-data lines. Returns parsed JSON values.
pub fn parse_sse_lines(raw: &str) -> Vec<Value> {
    let mut results = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if !line.starts_with("data:") {
            continue;
        }
        let json_text = line[5..].trim();
        if json_text.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(json_text) {
            Ok(data) => results.push(data),
            Err(e) => warn!(error = %e, "SSE parse error"),
        }
    }
    results
}

// === Part Types ===

/// A parsed Google Generative AI part.
#[derive(Debug, Clone)]
pub enum GooglePart {
    /// Thinking text with optional signature.
    Thinking { text: String, signature: String },
    /// Regular text content.
    Text(String),
    /// Function call (tool use).
    FunctionCall {
        id: Option<String>,
        name: String,
        args: Value,
        thought_signature: String,
    },
    /// Inline image data.
    InlineData { mime_type: String, data: String },
}

/// Extracts parts from a Google SSE data payload.
fn extract_parts(data: &Value) -> (Vec<GooglePart>, Option<&str>, Option<&Value>) {
    let response = data.get("response").unwrap_or(data);
    let candidates = response.get("candidates").and_then(|v| v.as_array());
    let first = candidates.and_then(|c| c.first());
    let content = first.and_then(|c| c.get("content"));
    let raw_parts = content
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array());
    let finish_reason = first
        .and_then(|c| c.get("finishReason"))
        .and_then(|v| v.as_str());
    let usage = response.get("usageMetadata");

    let mut parts = Vec::new();
    if let Some(raw) = raw_parts {
        for part in raw {
            if let Some(parsed) = parse_part(part) {
                parts.push(parsed);
            }
        }
    }
    (parts, finish_reason, usage)
}

/// Parses a single Google part JSON value.
fn parse_part(part: &Value) -> Option<GooglePart> {
    // Thinking block: {text, thought: true, thoughtSignature?}
    if part.get("thought").and_then(|v| v.as_bool()) == Some(true) {
        let text = part
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let sig = part
            .get("thoughtSignature")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(GooglePart::Thinking {
            text,
            signature: sig,
        });
    }

    // Regular text: {text}
    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
        return Some(GooglePart::Text(text.to_string()));
    }

    // Function call: {functionCall: {name, args, id?}}
    if let Some(fc) = part.get("functionCall") {
        let name = fc
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let args = fc.get("args").cloned().unwrap_or(json!({}));
        let id = fc.get("id").and_then(|v| v.as_str()).map(String::from);
        let sig = part
            .get("thoughtSignature")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(GooglePart::FunctionCall {
            id,
            name,
            args,
            thought_signature: sig,
        });
    }

    // Inline data: {inlineData: {mimeType, data}}
    if let Some(inline) = part.get("inlineData") {
        let mime = inline
            .get("mimeType")
            .and_then(|v| v.as_str())
            .unwrap_or("image/jpeg")
            .to_string();
        let data = inline
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some(GooglePart::InlineData {
            mime_type: mime,
            data,
        });
    }

    None
}

// === Usage Metadata ===

/// Extracted usage metadata from a Google response.
#[derive(Debug, Clone, Default)]
pub struct UsageMetadata {
    pub prompt_token_count: i64,
    pub candidates_token_count: i64,
    pub cached_content_token_count: i64,
}

impl UsageMetadata {
    fn from_value(v: &Value) -> Self {
        Self {
            prompt_token_count: v
                .get("promptTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            candidates_token_count: v
                .get("candidatesTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            cached_content_token_count: v
                .get("cachedContentTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        }
    }

    /// Anthropic input_tokens = promptTokenCount - cachedContentTokenCount
    pub fn input_tokens(&self) -> i64 {
        self.prompt_token_count - self.cached_content_token_count
    }

    pub fn output_tokens(&self) -> i64 {
        self.candidates_token_count
    }
}

// === Finish Reason Mapping ===

/// Maps Google finish reason to Anthropic stop reason.
fn map_stop_reason(finish_reason: Option<&str>, has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        return "tool_use";
    }
    match finish_reason {
        Some("MAX_TOKENS") => "max_tokens",
        Some("TOOL_USE") => "tool_use",
        _ => "end_turn",
    }
}

// === Signature Cache ===

/// In-memory cache for thinking signatures, keyed by tool call ID.
#[derive(Debug, Default)]
pub struct SignatureCache {
    /// tool_id -> signature
    tool_signatures: HashMap<String, String>,
    /// signature -> model family
    thinking_signatures: HashMap<String, String>,
}

impl SignatureCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Caches a tool call's thought signature for later retrieval.
    pub fn cache_tool_signature(&mut self, tool_id: &str, signature: &str) {
        if signature.len() >= MIN_SIGNATURE_LENGTH {
            self.tool_signatures
                .insert(tool_id.to_string(), signature.to_string());
        }
    }

    /// Caches a thinking signature with its model family.
    pub fn cache_thinking_signature(&mut self, signature: &str, model: &str) {
        if signature.len() >= MIN_SIGNATURE_LENGTH {
            let family = match get_model_family(model) {
                ModelFamily::Claude => "claude",
                ModelFamily::Gemini => "gemini",
                ModelFamily::Unknown => "unknown",
            };
            self.thinking_signatures
                .insert(signature.to_string(), family.to_string());
        }
    }
}

// === Collected Response (non-streaming) ===

/// Converts a complete Google response to an Anthropic Messages API response.
///
/// Used for non-streaming (collected) mode.
pub fn convert_collected_response(
    data: &Value,
    model: &str,
    sig_cache: &mut SignatureCache,
) -> Value {
    let (parts, finish_reason, usage_val) = extract_parts(data);
    let usage = usage_val.map(UsageMetadata::from_value).unwrap_or_default();

    let mut content = Vec::new();
    let mut has_tool_calls = false;

    for part in &parts {
        match part {
            GooglePart::Thinking { text, signature } => {
                if signature.len() >= MIN_SIGNATURE_LENGTH {
                    sig_cache.cache_thinking_signature(signature, model);
                }
                content.push(json!({
                    "type": "thinking",
                    "thinking": text,
                    "signature": signature
                }));
            }
            GooglePart::Text(text) => {
                content.push(json!({
                    "type": "text",
                    "text": text
                }));
            }
            GooglePart::FunctionCall {
                id,
                name,
                args,
                thought_signature,
            } => {
                let tool_id = id.clone().unwrap_or_else(generate_tool_id);
                let mut block = json!({
                    "type": "tool_use",
                    "id": tool_id,
                    "name": name,
                    "input": args
                });
                if thought_signature.len() >= MIN_SIGNATURE_LENGTH {
                    block
                        .as_object_mut()
                        .unwrap()
                        .insert("thoughtSignature".to_string(), json!(thought_signature));
                    sig_cache.cache_tool_signature(&tool_id, thought_signature);
                }
                content.push(block);
                has_tool_calls = true;
            }
            GooglePart::InlineData { mime_type, data } => {
                content.push(json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": mime_type,
                        "data": data
                    }
                }));
            }
        }
    }

    if content.is_empty() {
        content.push(json!({"type": "text", "text": ""}));
    }

    let stop_reason = map_stop_reason(finish_reason, has_tool_calls);

    json!({
        "id": generate_msg_id(),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": usage.input_tokens(),
            "output_tokens": usage.output_tokens(),
            "cache_read_input_tokens": usage.cached_content_token_count,
            "cache_creation_input_tokens": 0
        }
    })
}

// === Streaming Mode ===

/// Block type tracker for the streaming state machine.
#[derive(Debug, Clone, PartialEq)]
enum BlockType {
    Thinking,
    Text,
    ToolUse,
    Image,
}

/// Streaming state machine that converts Google SSE chunks to Anthropic SSE events.
///
/// Feed it SSE data lines via `process_sse_data` and collect the emitted
/// Anthropic events. Call `finish` when the stream ends to close any open blocks
/// and emit `message_delta` + `message_stop`.
#[derive(Debug)]
pub struct StreamingParser {
    model: String,
    message_id: String,
    has_emitted_start: bool,
    block_index: i32,
    current_block: Option<BlockType>,
    current_thinking_signature: String,
    usage: UsageMetadata,
    stop_reason: Option<String>,
    pub sig_cache: SignatureCache,
}

impl StreamingParser {
    /// Creates a new streaming parser for the given model.
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            message_id: generate_msg_id(),
            has_emitted_start: false,
            block_index: 0,
            current_block: None,
            current_thinking_signature: String::new(),
            usage: UsageMetadata::default(),
            stop_reason: None,
            sig_cache: SignatureCache::new(),
        }
    }

    /// Processes a single SSE data payload and returns Anthropic events.
    pub fn process_sse_data(&mut self, data: &Value) -> Vec<Value> {
        let mut events = Vec::new();
        let response = data.get("response").unwrap_or(data);

        // Update usage
        if let Some(u) = response.get("usageMetadata") {
            let new_usage = UsageMetadata::from_value(u);
            if new_usage.prompt_token_count > 0 {
                self.usage.prompt_token_count = new_usage.prompt_token_count;
            }
            if new_usage.candidates_token_count > 0 {
                self.usage.candidates_token_count = new_usage.candidates_token_count;
            }
            if new_usage.cached_content_token_count > 0 {
                self.usage.cached_content_token_count = new_usage.cached_content_token_count;
            }
        }

        let (parts, finish_reason, _) = extract_parts(data);

        // Emit message_start on first data with parts
        if !self.has_emitted_start && !parts.is_empty() {
            self.has_emitted_start = true;
            events.push(json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": self.usage.input_tokens(),
                        "output_tokens": 0,
                        "cache_read_input_tokens": self.usage.cached_content_token_count,
                        "cache_creation_input_tokens": 0
                    }
                }
            }));
        }

        for part in &parts {
            match part {
                GooglePart::Thinking { text, signature } => {
                    self.emit_thinking(&mut events, text, signature);
                }
                GooglePart::Text(text) => {
                    if !text.is_empty() {
                        self.emit_text(&mut events, text);
                    }
                }
                GooglePart::FunctionCall {
                    id,
                    name,
                    args,
                    thought_signature,
                } => {
                    self.emit_function_call(
                        &mut events,
                        id.as_deref(),
                        name,
                        args,
                        thought_signature,
                    );
                }
                GooglePart::InlineData { mime_type, data } => {
                    self.emit_inline_data(&mut events, mime_type, data);
                }
            }
        }

        // Check finish reason
        if let Some(reason) = finish_reason {
            if self.stop_reason.is_none() {
                match reason {
                    "MAX_TOKENS" => self.stop_reason = Some("max_tokens".to_string()),
                    "STOP" => self.stop_reason = Some("end_turn".to_string()),
                    _ => {}
                }
            }
        }

        events
    }

    fn emit_thinking(&mut self, events: &mut Vec<Value>, text: &str, signature: &str) {
        if self.current_block != Some(BlockType::Thinking) {
            if self.current_block.is_some() {
                self.close_block(events);
            }
            self.current_block = Some(BlockType::Thinking);
            self.current_thinking_signature.clear();
            events.push(json!({
                "type": "content_block_start",
                "index": self.block_index,
                "content_block": {"type": "thinking", "thinking": ""}
            }));
        }

        if signature.len() >= MIN_SIGNATURE_LENGTH {
            self.current_thinking_signature = signature.to_string();
            self.sig_cache
                .cache_thinking_signature(signature, &self.model);
        }

        events.push(json!({
            "type": "content_block_delta",
            "index": self.block_index,
            "delta": {"type": "thinking_delta", "thinking": text}
        }));
    }

    fn emit_text(&mut self, events: &mut Vec<Value>, text: &str) {
        if self.current_block != Some(BlockType::Text) {
            self.flush_thinking_signature(events);
            if self.current_block.is_some() {
                self.close_block(events);
            }
            self.current_block = Some(BlockType::Text);
            events.push(json!({
                "type": "content_block_start",
                "index": self.block_index,
                "content_block": {"type": "text", "text": ""}
            }));
        }

        events.push(json!({
            "type": "content_block_delta",
            "index": self.block_index,
            "delta": {"type": "text_delta", "text": text}
        }));
    }

    fn emit_function_call(
        &mut self,
        events: &mut Vec<Value>,
        id: Option<&str>,
        name: &str,
        args: &Value,
        thought_signature: &str,
    ) {
        self.flush_thinking_signature(events);
        if self.current_block.is_some() {
            self.close_block(events);
        }
        self.current_block = Some(BlockType::ToolUse);
        self.stop_reason = Some("tool_use".to_string());

        let tool_id = id.map(String::from).unwrap_or_else(generate_tool_id);

        let mut tool_block = json!({
            "type": "tool_use",
            "id": tool_id,
            "name": name,
            "input": {}
        });

        if thought_signature.len() >= MIN_SIGNATURE_LENGTH {
            tool_block
                .as_object_mut()
                .unwrap()
                .insert("thoughtSignature".to_string(), json!(thought_signature));
            self.sig_cache
                .cache_tool_signature(&tool_id, thought_signature);
        }

        events.push(json!({
            "type": "content_block_start",
            "index": self.block_index,
            "content_block": tool_block
        }));

        let args_json = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
        events.push(json!({
            "type": "content_block_delta",
            "index": self.block_index,
            "delta": {"type": "input_json_delta", "partial_json": args_json}
        }));
    }

    fn emit_inline_data(&mut self, events: &mut Vec<Value>, mime_type: &str, data: &str) {
        self.flush_thinking_signature(events);
        if self.current_block.is_some() {
            self.close_block(events);
        }
        self.current_block = Some(BlockType::Image);

        events.push(json!({
            "type": "content_block_start",
            "index": self.block_index,
            "content_block": {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime_type,
                    "data": data
                }
            }
        }));

        // Image blocks are immediately closed
        events.push(json!({"type": "content_block_stop", "index": self.block_index}));
        self.block_index += 1;
        self.current_block = None;
    }

    /// Emits a signature_delta if there's a pending thinking signature.
    fn flush_thinking_signature(&mut self, events: &mut Vec<Value>) {
        if self.current_block == Some(BlockType::Thinking)
            && !self.current_thinking_signature.is_empty()
        {
            events.push(json!({
                "type": "content_block_delta",
                "index": self.block_index,
                "delta": {"type": "signature_delta", "signature": self.current_thinking_signature}
            }));
            self.current_thinking_signature.clear();
        }
    }

    /// Closes the current open block.
    fn close_block(&mut self, events: &mut Vec<Value>) {
        events.push(json!({"type": "content_block_stop", "index": self.block_index}));
        self.block_index += 1;
    }

    /// Finishes the stream, closing any open blocks and emitting final events.
    ///
    /// Returns the final `message_delta` and `message_stop` events.
    pub fn finish(&mut self) -> Vec<Value> {
        let mut events = Vec::new();

        if !self.has_emitted_start {
            debug!("Stream finished with no content emitted");
            return events;
        }

        // Close any open block
        if self.current_block.is_some() {
            self.flush_thinking_signature(&mut events);
            self.close_block(&mut events);
        }

        let stop = self.stop_reason.as_deref().unwrap_or("end_turn");
        events.push(json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop, "stop_sequence": null},
            "usage": {
                "output_tokens": self.usage.output_tokens(),
                "cache_read_input_tokens": self.usage.cached_content_token_count,
                "cache_creation_input_tokens": 0
            }
        }));

        events.push(json!({"type": "message_stop"}));
        events
    }

    /// Returns whether any content has been emitted.
    pub fn has_content(&self) -> bool {
        self.has_emitted_start
    }
}

// === ID Generation ===

/// Generates a random message ID in Anthropic format.
fn generate_msg_id() -> String {
    format!("msg_{:032x}", rand_u128())
}

/// Generates a random tool use ID in Anthropic format.
fn generate_tool_id() -> String {
    format!("toolu_{:024x}", rand_u128() & ((1u128 << 96) - 1))
}

/// Simple random u128 using std (no external crate needed for IDs).
fn rand_u128() -> u128 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u8(0);
    let a = h.finish() as u128;
    let s2 = RandomState::new();
    let mut h2 = s2.build_hasher();
    h2.write_u8(1);
    let b = h2.finish() as u128;
    (a << 64) | b
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === parse_sse_lines ===

    #[test]
    fn test_parse_sse_lines_basic() {
        let raw = "data: {\"response\": {\"candidates\": []}}\n\ndata: {\"done\": true}\n";
        let results = parse_sse_lines(raw);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_parse_sse_lines_skips_non_data() {
        let raw = "event: message\ndata: {\"ok\": true}\n: comment\n";
        let results = parse_sse_lines(raw);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["ok"], true);
    }

    #[test]
    fn test_parse_sse_lines_skips_empty_data() {
        let raw = "data: \ndata: {\"ok\": true}\n";
        let results = parse_sse_lines(raw);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parse_sse_lines_invalid_json() {
        let raw = "data: not-json\ndata: {\"ok\": true}\n";
        let results = parse_sse_lines(raw);
        assert_eq!(results.len(), 1);
    }

    // === parse_part ===

    #[test]
    fn test_parse_part_text() {
        let part = json!({"text": "Hello"});
        match parse_part(&part).unwrap() {
            GooglePart::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("Expected Text"),
        }
    }

    #[test]
    fn test_parse_part_thinking() {
        let sig = "a".repeat(MIN_SIGNATURE_LENGTH);
        let part = json!({"text": "thinking...", "thought": true, "thoughtSignature": sig});
        match parse_part(&part).unwrap() {
            GooglePart::Thinking { text, signature } => {
                assert_eq!(text, "thinking...");
                assert_eq!(signature, sig);
            }
            _ => panic!("Expected Thinking"),
        }
    }

    #[test]
    fn test_parse_part_function_call() {
        let part = json!({
            "functionCall": {"name": "read_file", "args": {"path": "/tmp"}, "id": "call_1"}
        });
        match parse_part(&part).unwrap() {
            GooglePart::FunctionCall { id, name, args, .. } => {
                assert_eq!(id, Some("call_1".to_string()));
                assert_eq!(name, "read_file");
                assert_eq!(args["path"], "/tmp");
            }
            _ => panic!("Expected FunctionCall"),
        }
    }

    #[test]
    fn test_parse_part_inline_data() {
        let part = json!({"inlineData": {"mimeType": "image/png", "data": "abc123"}});
        match parse_part(&part).unwrap() {
            GooglePart::InlineData { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data, "abc123");
            }
            _ => panic!("Expected InlineData"),
        }
    }

    #[test]
    fn test_parse_part_unknown() {
        let part = json!({"unknownField": true});
        assert!(parse_part(&part).is_none());
    }

    // === UsageMetadata ===

    #[test]
    fn test_usage_metadata() {
        let v = json!({
            "promptTokenCount": 100,
            "candidatesTokenCount": 50,
            "cachedContentTokenCount": 20
        });
        let usage = UsageMetadata::from_value(&v);
        assert_eq!(usage.input_tokens(), 80); // 100 - 20
        assert_eq!(usage.output_tokens(), 50);
    }

    #[test]
    fn test_usage_metadata_defaults() {
        let usage = UsageMetadata::from_value(&json!({}));
        assert_eq!(usage.input_tokens(), 0);
        assert_eq!(usage.output_tokens(), 0);
    }

    // === map_stop_reason ===

    #[test]
    fn test_stop_reason_mapping() {
        assert_eq!(map_stop_reason(Some("STOP"), false), "end_turn");
        assert_eq!(map_stop_reason(Some("MAX_TOKENS"), false), "max_tokens");
        assert_eq!(map_stop_reason(Some("TOOL_USE"), false), "tool_use");
        assert_eq!(map_stop_reason(None, false), "end_turn");
        // tool_calls override finish_reason
        assert_eq!(map_stop_reason(Some("STOP"), true), "tool_use");
    }

    // === convert_collected_response ===

    #[test]
    fn test_collected_text_response() {
        let data = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello world"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "cachedContentTokenCount": 0
            }
        });
        let mut cache = SignatureCache::new();
        let result = convert_collected_response(&data, "claude-sonnet-4-5", &mut cache);

        assert_eq!(result["role"], "assistant");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello world");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_collected_tool_use_response() {
        let data = json!({
            "candidates": [{
                "content": {"parts": [{
                    "functionCall": {"name": "read_file", "args": {"path": "/tmp"}, "id": "call_1"}
                }]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5}
        });
        let mut cache = SignatureCache::new();
        let result = convert_collected_response(&data, "claude-sonnet-4-5", &mut cache);

        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "read_file");
        assert_eq!(result["content"][0]["id"], "call_1");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_collected_thinking_response() {
        let sig = "a".repeat(MIN_SIGNATURE_LENGTH);
        let data = json!({
            "candidates": [{
                "content": {"parts": [
                    {"text": "Let me think...", "thought": true, "thoughtSignature": sig},
                    {"text": "The answer is 42"}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 20}
        });
        let mut cache = SignatureCache::new();
        let result = convert_collected_response(&data, "gemini-3-flash", &mut cache);

        assert_eq!(result["content"][0]["type"], "thinking");
        assert_eq!(result["content"][0]["thinking"], "Let me think...");
        assert_eq!(result["content"][0]["signature"], sig);
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "The answer is 42");
    }

    #[test]
    fn test_collected_empty_response() {
        let data = json!({
            "candidates": [{"content": {"parts": []}, "finishReason": "STOP"}],
            "usageMetadata": {}
        });
        let mut cache = SignatureCache::new();
        let result = convert_collected_response(&data, "claude-sonnet-4-5", &mut cache);

        // Should have fallback empty text
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "");
    }

    #[test]
    fn test_collected_response_wrapper() {
        // Test with {"response": {...}} wrapper
        let data = json!({
            "response": {
                "candidates": [{
                    "content": {"parts": [{"text": "wrapped"}]},
                    "finishReason": "STOP"
                }],
                "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2}
            }
        });
        let mut cache = SignatureCache::new();
        let result = convert_collected_response(&data, "claude-sonnet-4-5", &mut cache);
        assert_eq!(result["content"][0]["text"], "wrapped");
    }

    // === StreamingParser ===

    #[test]
    fn test_streaming_text() {
        let mut parser = StreamingParser::new("claude-sonnet-4-5");

        let data = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 3}
        });

        let events = parser.process_sse_data(&data);
        // message_start + content_block_start + content_block_delta
        assert!(events.len() >= 3);
        assert_eq!(events[0]["type"], "message_start");
        assert_eq!(events[1]["type"], "content_block_start");
        assert_eq!(events[1]["content_block"]["type"], "text");
        assert_eq!(events[2]["type"], "content_block_delta");
        assert_eq!(events[2]["delta"]["text"], "Hello");

        let final_events = parser.finish();
        assert_eq!(final_events[0]["type"], "content_block_stop");
        assert_eq!(final_events[1]["type"], "message_delta");
        assert_eq!(final_events[1]["delta"]["stop_reason"], "end_turn");
        assert_eq!(final_events[2]["type"], "message_stop");
    }

    #[test]
    fn test_streaming_thinking_then_text() {
        let mut parser = StreamingParser::new("gemini-3-flash");
        let sig = "b".repeat(MIN_SIGNATURE_LENGTH);

        // Thinking chunk
        let data1 = json!({
            "candidates": [{
                "content": {"parts": [
                    {"text": "thinking...", "thought": true, "thoughtSignature": sig}
                ]}
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5}
        });
        let events1 = parser.process_sse_data(&data1);
        assert_eq!(events1[0]["type"], "message_start");
        assert_eq!(events1[1]["type"], "content_block_start");
        assert_eq!(events1[1]["content_block"]["type"], "thinking");
        assert_eq!(events1[2]["type"], "content_block_delta");
        assert_eq!(events1[2]["delta"]["type"], "thinking_delta");

        // Text chunk
        let data2 = json!({
            "candidates": [{
                "content": {"parts": [{"text": "Answer"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 10}
        });
        let events2 = parser.process_sse_data(&data2);
        // Should emit: signature_delta, content_block_stop, content_block_start(text), content_block_delta(text)
        let types: Vec<&str> = events2
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect();
        assert!(types.contains(&"content_block_delta"));
        // Find the signature_delta
        let sig_event = events2.iter().find(|e| {
            e.get("delta")
                .and_then(|d| d.get("type"))
                .and_then(|t| t.as_str())
                == Some("signature_delta")
        });
        assert!(sig_event.is_some());
    }

    #[test]
    fn test_streaming_tool_use() {
        let mut parser = StreamingParser::new("claude-sonnet-4-5");

        let data = json!({
            "candidates": [{
                "content": {"parts": [{
                    "functionCall": {"name": "bash", "args": {"cmd": "ls"}, "id": "call_1"}
                }]}
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5}
        });

        let events = parser.process_sse_data(&data);
        assert_eq!(events[0]["type"], "message_start");
        assert_eq!(events[1]["type"], "content_block_start");
        assert_eq!(events[1]["content_block"]["type"], "tool_use");
        assert_eq!(events[1]["content_block"]["name"], "bash");
        assert_eq!(events[1]["content_block"]["id"], "call_1");
        assert_eq!(events[2]["type"], "content_block_delta");
        assert_eq!(events[2]["delta"]["type"], "input_json_delta");

        let final_events = parser.finish();
        let msg_delta = &final_events[1];
        assert_eq!(msg_delta["delta"]["stop_reason"], "tool_use");
    }

    #[test]
    fn test_streaming_image() {
        let mut parser = StreamingParser::new("gemini-3-flash");

        let data = json!({
            "candidates": [{
                "content": {"parts": [{
                    "inlineData": {"mimeType": "image/png", "data": "abc123"}
                }]}
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2}
        });

        let events = parser.process_sse_data(&data);
        // message_start + content_block_start(image) + content_block_stop
        assert_eq!(events[1]["type"], "content_block_start");
        assert_eq!(events[1]["content_block"]["type"], "image");
        assert_eq!(events[1]["content_block"]["source"]["data"], "abc123");
        assert_eq!(events[2]["type"], "content_block_stop");
    }

    #[test]
    fn test_streaming_no_content() {
        let mut parser = StreamingParser::new("claude-sonnet-4-5");
        let final_events = parser.finish();
        assert!(final_events.is_empty());
        assert!(!parser.has_content());
    }

    #[test]
    fn test_streaming_usage_accumulation() {
        let mut parser = StreamingParser::new("claude-sonnet-4-5");

        // First chunk with initial usage
        let data1 = json!({
            "candidates": [{"content": {"parts": [{"text": "Hi"}]}}],
            "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 5, "cachedContentTokenCount": 20}
        });
        parser.process_sse_data(&data1);

        // Second chunk with updated output tokens
        let data2 = json!({
            "candidates": [{"content": {"parts": [{"text": " there"}]}}],
            "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 15, "cachedContentTokenCount": 20}
        });
        parser.process_sse_data(&data2);

        let final_events = parser.finish();
        let msg_delta = &final_events[1];
        assert_eq!(msg_delta["usage"]["output_tokens"], 15);
        assert_eq!(msg_delta["usage"]["cache_read_input_tokens"], 20);
    }

    // === SignatureCache ===

    #[test]
    fn test_signature_cache_tool() {
        let mut cache = SignatureCache::new();
        let sig = "x".repeat(MIN_SIGNATURE_LENGTH);
        cache.cache_tool_signature("call_1", &sig);
        assert!(cache.tool_signatures.contains_key("call_1"));
    }

    #[test]
    fn test_signature_cache_short_signature_ignored() {
        let mut cache = SignatureCache::new();
        cache.cache_tool_signature("call_1", "short");
        assert!(!cache.tool_signatures.contains_key("call_1"));
    }

    #[test]
    fn test_signature_cache_thinking() {
        let mut cache = SignatureCache::new();
        let sig = "y".repeat(MIN_SIGNATURE_LENGTH);
        cache.cache_thinking_signature(&sig, "gemini-3-flash");
        assert_eq!(cache.thinking_signatures.get(&sig).unwrap(), "gemini");
    }

    // === ID generation ===

    #[test]
    fn test_generate_msg_id_format() {
        let id = generate_msg_id();
        assert!(id.starts_with("msg_"));
        assert!(id.len() > 4);
    }

    #[test]
    fn test_generate_tool_id_format() {
        let id = generate_tool_id();
        assert!(id.starts_with("toolu_"));
        assert!(id.len() > 6);
    }

    #[test]
    fn test_ids_are_unique() {
        let id1 = generate_msg_id();
        let id2 = generate_msg_id();
        assert_ne!(id1, id2);
    }
}
