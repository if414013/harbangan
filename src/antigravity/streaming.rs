//! Cloud Code SSE streaming parser.
//!
//! Parses SSE responses from the Cloud Code API and converts them to
//! Anthropic Messages API format. Supports both streaming (emitting SSE
//! events incrementally) and collected (accumulating into a single response)
//! modes.

use serde_json::{json, Value};
use tracing::debug;
use uuid::Uuid;

/// Minimum length for a valid thinking signature.
const MIN_SIGNATURE_LENGTH: usize = 50;

// === SSE Line Parsing ===

/// Extracts the JSON payload from an SSE `data:` line.
///
/// Returns `None` if the line doesn't start with `data:` or the payload is empty.
fn parse_sse_data_line(line: &str) -> Option<Value> {
    let json_text = line.strip_prefix("data:")?.trim();
    if json_text.is_empty() {
        return None;
    }
    serde_json::from_str(json_text).map_err(|e| {
        debug!(json_text = json_text, error = %e, "Failed to parse SSE data JSON");
        e
    }).ok()
}

/// Extracts the inner response, candidates, and parts from a parsed SSE JSON value.
fn extract_parts(data: &Value) -> (&Value, Vec<&Value>) {
    let inner = data.get("response").unwrap_or(data);
    let parts = inner
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default();
    (inner, parts)
}

// === Usage Metadata ===

/// Token usage extracted from Cloud Code response.
#[derive(Debug, Clone, Default)]
pub struct UsageMetadata {
    pub prompt_token_count: i64,
    pub candidates_token_count: i64,
    pub cached_content_token_count: i64,
}

impl UsageMetadata {
    /// Anthropic `input_tokens` = promptTokenCount - cachedContentTokenCount.
    pub fn input_tokens(&self) -> i64 {
        (self.prompt_token_count - self.cached_content_token_count).max(0)
    }

    /// Anthropic `output_tokens` = candidatesTokenCount.
    pub fn output_tokens(&self) -> i64 {
        self.candidates_token_count
    }

    /// Update from a Cloud Code usageMetadata JSON object.
    fn update_from(&mut self, usage: &Value) {
        if let Some(v) = usage.get("promptTokenCount").and_then(|v| v.as_i64()) {
            self.prompt_token_count = v;
        }
        if let Some(v) = usage.get("candidatesTokenCount").and_then(|v| v.as_i64()) {
            self.candidates_token_count = v;
        }
        if let Some(v) = usage
            .get("cachedContentTokenCount")
            .and_then(|v| v.as_i64())
        {
            self.cached_content_token_count = v;
        }
    }
}

// === Finish Reason Mapping ===

/// Maps Cloud Code finish reason to Anthropic stop_reason.
fn map_finish_reason(finish_reason: Option<&str>, has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        return "tool_use";
    }
    match finish_reason {
        Some("MAX_TOKENS") => "max_tokens",
        Some("TOOL_USE") => "tool_use",
        _ => "end_turn",
    }
}

/// Extracts the finish reason string from a candidate.
fn get_finish_reason(data: &Value) -> Option<String> {
    data.get("response")
        .unwrap_or(data)
        .pointer("/candidates/0/finishReason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// === Message ID Generation ===

/// Generates a message ID in Anthropic format: `msg_<hex>`.
fn generate_message_id() -> String {
    format!("msg_{}", Uuid::new_v4().as_simple())
}

/// Generates a tool use ID in Anthropic format: `toolu_<hex>`.
fn generate_tool_id() -> String {
    format!("toolu_{}", Uuid::new_v4().as_simple())
}

// === Anthropic SSE Event Types ===

/// An Anthropic-format SSE event emitted during streaming.
#[derive(Debug, Clone)]
pub enum AnthropicSseEvent {
    MessageStart(Value),
    ContentBlockStart { index: usize, content_block: Value },
    ContentBlockDelta { index: usize, delta: Value },
    ContentBlockStop { index: usize },
    MessageDelta(Value),
    MessageStop,
}

impl AnthropicSseEvent {
    /// Serializes the event to an SSE `event:` + `data:` pair.
    pub fn to_sse_string(&self) -> String {
        match self {
            Self::MessageStart(msg) => {
                let data = json!({ "type": "message_start", "message": msg });
                format!("event: message_start\ndata: {}\n\n", data)
            }
            Self::ContentBlockStart {
                index,
                content_block,
            } => {
                let data = json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": content_block
                });
                format!("event: content_block_start\ndata: {}\n\n", data)
            }
            Self::ContentBlockDelta { index, delta } => {
                let data = json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": delta
                });
                format!("event: content_block_delta\ndata: {}\n\n", data)
            }
            Self::ContentBlockStop { index } => {
                let data = json!({ "type": "content_block_stop", "index": index });
                format!("event: content_block_stop\ndata: {}\n\n", data)
            }
            Self::MessageDelta(delta) => {
                let data = json!({ "type": "message_delta", "delta": delta.get("delta").cloned().unwrap_or(json!({})), "usage": delta.get("usage").cloned().unwrap_or(json!({})) });
                format!("event: message_delta\ndata: {}\n\n", data)
            }
            Self::MessageStop => {
                let data = json!({ "type": "message_stop" });
                format!("event: message_stop\ndata: {}\n\n", data)
            }
        }
    }
}

// === Block Type Tracking ===

/// Tracks the current content block type during streaming.
#[derive(Debug, Clone, PartialEq)]
enum BlockType {
    Thinking,
    Text,
    ToolUse,
    Image,
}

// === Streaming SSE Parser ===

/// Parses Cloud Code SSE responses and emits Anthropic-format events.
///
/// Handles thinking blocks with signature caching, text blocks,
/// functionCall (tool_use) blocks, and inlineData (image) blocks.
pub struct CloudCodeSseParser {
    message_id: String,
    model: String,
    block_index: usize,
    current_block_type: Option<BlockType>,
    current_thinking_signature: String,
    usage: UsageMetadata,
    stop_reason: Option<String>,
    has_emitted_start: bool,
    has_tool_calls: bool,
}

impl CloudCodeSseParser {
    /// Creates a new parser for the given model.
    pub fn new(model: &str) -> Self {
        Self {
            message_id: generate_message_id(),
            model: model.to_string(),
            block_index: 0,
            current_block_type: None,
            current_thinking_signature: String::new(),
            usage: UsageMetadata::default(),
            stop_reason: None,
            has_emitted_start: false,
            has_tool_calls: false,
        }
    }

    /// Processes a single SSE `data:` line and returns any events to emit.
    ///
    /// Call this for each line received from the Cloud Code SSE stream.
    pub fn process_line(&mut self, line: &str) -> Vec<AnthropicSseEvent> {
        let data = match parse_sse_data_line(line) {
            Some(d) => d,
            None => return vec![],
        };

        let mut events = Vec::new();

        // Extract usage metadata
        let inner = data.get("response").unwrap_or(&data);
        if let Some(usage) = inner.get("usageMetadata") {
            self.usage.update_from(usage);
        }

        // Extract finish reason
        if let Some(reason) = get_finish_reason(&data) {
            if self.stop_reason.is_none() || reason == "TOOL_USE" {
                self.stop_reason = Some(reason);
            }
        }

        let (_, parts) = extract_parts(&data);
        if parts.is_empty() {
            return events;
        }

        // Emit message_start on first data with parts
        if !self.has_emitted_start {
            self.has_emitted_start = true;
            events.push(AnthropicSseEvent::MessageStart(json!({
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
            })));
        }

        // Process each part
        for part in &parts {
            let part_events = self.process_part(part);
            events.extend(part_events);
        }

        events
    }

    /// Processes a single part from the Cloud Code response.
    fn process_part(&mut self, part: &Value) -> Vec<AnthropicSseEvent> {
        if part.get("thought") == Some(&json!(true)) {
            self.process_thinking_part(part)
        } else if part.get("text").is_some() {
            self.process_text_part(part)
        } else if part.get("functionCall").is_some() {
            self.process_function_call_part(part)
        } else if part.get("inlineData").is_some() {
            self.process_inline_data_part(part)
        } else {
            vec![]
        }
    }

    /// Handles a thinking part (`{text, thought: true, thoughtSignature}`).
    fn process_thinking_part(&mut self, part: &Value) -> Vec<AnthropicSseEvent> {
        let mut events = Vec::new();
        let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let signature = part
            .get("thoughtSignature")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if self.current_block_type != Some(BlockType::Thinking) {
            if self.current_block_type.is_some() {
                events.push(AnthropicSseEvent::ContentBlockStop {
                    index: self.block_index,
                });
                self.block_index += 1;
            }
            self.current_block_type = Some(BlockType::Thinking);
            self.current_thinking_signature.clear();
            events.push(AnthropicSseEvent::ContentBlockStart {
                index: self.block_index,
                content_block: json!({ "type": "thinking", "thinking": "" }),
            });
        }

        if signature.len() >= MIN_SIGNATURE_LENGTH {
            self.current_thinking_signature = signature.to_string();
        }

        events.push(AnthropicSseEvent::ContentBlockDelta {
            index: self.block_index,
            delta: json!({ "type": "thinking_delta", "thinking": text }),
        });

        events
    }

    /// Emits a signature_delta event if there's a pending thinking signature.
    fn flush_thinking_signature(&mut self) -> Vec<AnthropicSseEvent> {
        let mut events = Vec::new();
        if self.current_block_type == Some(BlockType::Thinking)
            && !self.current_thinking_signature.is_empty()
        {
            events.push(AnthropicSseEvent::ContentBlockDelta {
                index: self.block_index,
                delta: json!({
                    "type": "signature_delta",
                    "signature": self.current_thinking_signature
                }),
            });
            self.current_thinking_signature.clear();
        }
        events
    }

    /// Handles a text part (`{text}`).
    fn process_text_part(&mut self, part: &Value) -> Vec<AnthropicSseEvent> {
        let mut events = Vec::new();
        let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");

        // Skip empty text parts
        if text.is_empty() {
            return events;
        }

        if self.current_block_type != Some(BlockType::Text) {
            events.extend(self.flush_thinking_signature());
            if self.current_block_type.is_some() {
                events.push(AnthropicSseEvent::ContentBlockStop {
                    index: self.block_index,
                });
                self.block_index += 1;
            }
            self.current_block_type = Some(BlockType::Text);
            events.push(AnthropicSseEvent::ContentBlockStart {
                index: self.block_index,
                content_block: json!({ "type": "text", "text": "" }),
            });
        }

        events.push(AnthropicSseEvent::ContentBlockDelta {
            index: self.block_index,
            delta: json!({ "type": "text_delta", "text": text }),
        });

        events
    }

    /// Handles a functionCall part.
    fn process_function_call_part(&mut self, part: &Value) -> Vec<AnthropicSseEvent> {
        let mut events = Vec::new();
        let fc = match part.get("functionCall") {
            Some(fc) => fc,
            None => return events,
        };

        let function_call_signature = part
            .get("thoughtSignature")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        events.extend(self.flush_thinking_signature());
        if self.current_block_type.is_some() {
            events.push(AnthropicSseEvent::ContentBlockStop {
                index: self.block_index,
            });
            self.block_index += 1;
        }
        self.current_block_type = Some(BlockType::ToolUse);
        self.has_tool_calls = true;

        let tool_id = fc
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(generate_tool_id);

        let mut tool_use_block = json!({
            "type": "tool_use",
            "id": tool_id,
            "name": fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
            "input": {}
        });

        if function_call_signature.len() >= MIN_SIGNATURE_LENGTH {
            tool_use_block.as_object_mut().unwrap().insert(
                "thoughtSignature".to_string(),
                json!(function_call_signature),
            );
        }

        events.push(AnthropicSseEvent::ContentBlockStart {
            index: self.block_index,
            content_block: tool_use_block,
        });

        let args = fc.get("args").cloned().unwrap_or(json!({}));
        events.push(AnthropicSseEvent::ContentBlockDelta {
            index: self.block_index,
            delta: json!({
                "type": "input_json_delta",
                "partial_json": args.to_string()
            }),
        });

        events
    }

    /// Handles an inlineData part (image).
    fn process_inline_data_part(&mut self, part: &Value) -> Vec<AnthropicSseEvent> {
        let mut events = Vec::new();
        let inline_data = match part.get("inlineData") {
            Some(d) => d,
            None => return events,
        };

        events.extend(self.flush_thinking_signature());
        if self.current_block_type.is_some() {
            events.push(AnthropicSseEvent::ContentBlockStop {
                index: self.block_index,
            });
            self.block_index += 1;
        }
        self.current_block_type = Some(BlockType::Image);

        let mime_type = inline_data
            .get("mimeType")
            .and_then(|v| v.as_str())
            .unwrap_or("image/jpeg");
        let data = inline_data
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        events.push(AnthropicSseEvent::ContentBlockStart {
            index: self.block_index,
            content_block: json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime_type,
                    "data": data
                }
            }),
        });

        events.push(AnthropicSseEvent::ContentBlockStop {
            index: self.block_index,
        });
        self.block_index += 1;
        self.current_block_type = None;

        events
    }

    /// Returns true if any content has been emitted.
    pub fn has_content(&self) -> bool {
        self.has_emitted_start
    }

    /// Finalizes the stream, emitting closing events.
    ///
    /// Call this after all SSE lines have been processed.
    pub fn finish(&mut self) -> Vec<AnthropicSseEvent> {
        let mut events = Vec::new();

        if !self.has_emitted_start {
            debug!("No content parts received from Cloud Code");
            return events;
        }

        // Close any open block
        if self.current_block_type.is_some() {
            events.extend(self.flush_thinking_signature());
            events.push(AnthropicSseEvent::ContentBlockStop {
                index: self.block_index,
            });
        }

        let stop_reason = map_finish_reason(self.stop_reason.as_deref(), self.has_tool_calls);

        events.push(AnthropicSseEvent::MessageDelta(json!({
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            },
            "usage": {
                "output_tokens": self.usage.output_tokens(),
                "cache_read_input_tokens": self.usage.cached_content_token_count,
                "cache_creation_input_tokens": 0
            }
        })));

        events.push(AnthropicSseEvent::MessageStop);

        events
    }
}

// === Handler Compatibility API ===

/// Parses SSE `data:` lines from a raw response body into JSON values.
///
/// Used by handlers to iterate over parsed SSE data objects.
pub fn parse_sse_lines(body: &str) -> Vec<Value> {
    body.lines().filter_map(parse_sse_data_line).collect()
}

/// Streaming parser with a handler-friendly API.
///
/// Wraps `CloudCodeSseParser` and returns serialized JSON strings
/// instead of typed events, matching the interface expected by handlers.
pub struct StreamingParser {
    inner: CloudCodeSseParser,
}

impl StreamingParser {
    /// Creates a new streaming parser for the given model.
    pub fn new(model: &str) -> Self {
        Self {
            inner: CloudCodeSseParser::new(model),
        }
    }

    /// Processes a parsed SSE data value and returns serialized Anthropic events.
    pub fn process_sse_data(&mut self, data: &Value) -> Vec<String> {
        // Re-encode as a data: line so the inner parser can process it
        let line = format!("data: {}", data);
        self.inner
            .process_line(&line)
            .into_iter()
            .map(|e| event_to_json(&e).to_string())
            .collect()
    }

    /// Finalizes the stream and returns serialized closing events.
    pub fn finish(&mut self) -> Vec<String> {
        self.inner
            .finish()
            .into_iter()
            .map(|e| event_to_json(&e).to_string())
            .collect()
    }
}

/// Converts an `AnthropicSseEvent` to its JSON representation.
fn event_to_json(event: &AnthropicSseEvent) -> Value {
    match event {
        AnthropicSseEvent::MessageStart(msg) => {
            json!({ "type": "message_start", "message": msg })
        }
        AnthropicSseEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            json!({ "type": "content_block_start", "index": index, "content_block": content_block })
        }
        AnthropicSseEvent::ContentBlockDelta { index, delta } => {
            json!({ "type": "content_block_delta", "index": index, "delta": delta })
        }
        AnthropicSseEvent::ContentBlockStop { index } => {
            json!({ "type": "content_block_stop", "index": index })
        }
        AnthropicSseEvent::MessageDelta(v) => {
            json!({
                "type": "message_delta",
                "delta": v.get("delta").cloned().unwrap_or(json!({})),
                "usage": v.get("usage").cloned().unwrap_or(json!({}))
            })
        }
        AnthropicSseEvent::MessageStop => {
            json!({ "type": "message_stop" })
        }
    }
}

// === Collected (Non-Streaming) Mode ===

/// Parses all SSE lines from a Cloud Code response and returns a single
/// Anthropic Messages API response JSON.
///
/// This is the "collected" mode used for non-streaming requests. It accumulates
/// all parts across SSE chunks, merges thinking text, and produces one response.
pub fn parse_collected_response(sse_body: &str, model: &str) -> Value {
    let mut accumulated_thinking_text = String::new();
    let mut accumulated_thinking_signature = String::new();
    let mut accumulated_text = String::new();
    let mut final_parts: Vec<Value> = Vec::new();
    let mut usage = UsageMetadata::default();
    let mut finish_reason: Option<String> = None;

    let flush_thinking = |parts: &mut Vec<Value>, text: &mut String, sig: &mut String| {
        if !text.is_empty() {
            parts.push(json!({
                "thought": true,
                "text": std::mem::take(text),
                "thoughtSignature": std::mem::take(sig)
            }));
        }
    };

    let flush_text = |parts: &mut Vec<Value>, text: &mut String| {
        if !text.is_empty() {
            parts.push(json!({ "text": std::mem::take(text) }));
        }
    };

    for line in sse_body.lines() {
        let data = match parse_sse_data_line(line) {
            Some(d) => d,
            None => continue,
        };

        let inner = data.get("response").unwrap_or(&data);

        if let Some(u) = inner.get("usageMetadata") {
            usage.update_from(u);
        }

        if let Some(reason) = get_finish_reason(&data) {
            finish_reason = Some(reason);
        }

        let (_, parts) = extract_parts(&data);
        for part in parts {
            if part.get("thought") == Some(&json!(true)) {
                flush_text(&mut final_parts, &mut accumulated_text);
                let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                accumulated_thinking_text.push_str(text);
                if let Some(sig) = part.get("thoughtSignature").and_then(|v| v.as_str()) {
                    if !sig.is_empty() {
                        accumulated_thinking_signature = sig.to_string();
                    }
                }
            } else if part.get("functionCall").is_some() {
                flush_thinking(
                    &mut final_parts,
                    &mut accumulated_thinking_text,
                    &mut accumulated_thinking_signature,
                );
                flush_text(&mut final_parts, &mut accumulated_text);
                final_parts.push(part.clone());
            } else if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                if text.is_empty() {
                    continue;
                }
                flush_thinking(
                    &mut final_parts,
                    &mut accumulated_thinking_text,
                    &mut accumulated_thinking_signature,
                );
                accumulated_text.push_str(text);
            } else if part.get("inlineData").is_some() {
                flush_thinking(
                    &mut final_parts,
                    &mut accumulated_thinking_text,
                    &mut accumulated_thinking_signature,
                );
                flush_text(&mut final_parts, &mut accumulated_text);
                final_parts.push(part.clone());
            }
        }
    }

    // Final flush
    flush_thinking(
        &mut final_parts,
        &mut accumulated_thinking_text,
        &mut accumulated_thinking_signature,
    );
    flush_text(&mut final_parts, &mut accumulated_text);

    // Convert accumulated Google parts to Anthropic content blocks
    let mut anthropic_content: Vec<Value> = Vec::new();
    let mut has_tool_calls = false;

    for part in &final_parts {
        if part.get("thought") == Some(&json!(true)) {
            let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let signature = part
                .get("thoughtSignature")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            anthropic_content.push(json!({
                "type": "thinking",
                "thinking": text,
                "signature": signature
            }));
        } else if let Some(fc) = part.get("functionCall") {
            let tool_id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(generate_tool_id);
            let mut block = json!({
                "type": "tool_use",
                "id": tool_id,
                "name": fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "input": fc.get("args").cloned().unwrap_or(json!({}))
            });
            // Include thoughtSignature from part level (Gemini 3+)
            if let Some(sig) = part.get("thoughtSignature").and_then(|v| v.as_str()) {
                if sig.len() >= MIN_SIGNATURE_LENGTH {
                    block
                        .as_object_mut()
                        .unwrap()
                        .insert("thoughtSignature".to_string(), json!(sig));
                }
            }
            anthropic_content.push(block);
            has_tool_calls = true;
        } else if let Some(inline) = part.get("inlineData") {
            let mime = inline
                .get("mimeType")
                .and_then(|v| v.as_str())
                .unwrap_or("image/jpeg");
            let data = inline.get("data").and_then(|v| v.as_str()).unwrap_or("");
            anthropic_content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime,
                    "data": data
                }
            }));
        } else if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
            anthropic_content.push(json!({
                "type": "text",
                "text": text
            }));
        }
    }

    if anthropic_content.is_empty() {
        anthropic_content.push(json!({ "type": "text", "text": "" }));
    }

    let stop_reason = map_finish_reason(finish_reason.as_deref(), has_tool_calls);

    json!({
        "id": generate_message_id(),
        "type": "message",
        "role": "assistant",
        "content": anthropic_content,
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

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === Helper: build SSE data line ===

    fn sse_line(data: &Value) -> String {
        format!("data: {}", data)
    }

    fn make_sse_chunk(
        parts: Vec<Value>,
        finish_reason: Option<&str>,
        usage: Option<Value>,
    ) -> String {
        let mut candidate = json!({ "content": { "parts": parts } });
        if let Some(reason) = finish_reason {
            candidate
                .as_object_mut()
                .unwrap()
                .insert("finishReason".to_string(), json!(reason));
        }
        let mut response = json!({ "candidates": [candidate] });
        if let Some(u) = usage {
            response
                .as_object_mut()
                .unwrap()
                .insert("usageMetadata".to_string(), u);
        }
        let data = json!({ "response": response });
        sse_line(&data)
    }

    fn valid_signature() -> String {
        "a".repeat(MIN_SIGNATURE_LENGTH)
    }

    // === parse_sse_data_line ===

    #[test]
    fn test_parse_sse_data_line_valid() {
        let line = r#"data: {"response": {}}"#;
        let result = parse_sse_data_line(line);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_sse_data_line_no_prefix() {
        assert!(parse_sse_data_line("event: message").is_none());
        assert!(parse_sse_data_line("").is_none());
        assert!(parse_sse_data_line("not data").is_none());
    }

    #[test]
    fn test_parse_sse_data_line_empty_payload() {
        assert!(parse_sse_data_line("data: ").is_none());
        assert!(parse_sse_data_line("data:").is_none());
    }

    #[test]
    fn test_parse_sse_data_line_invalid_json() {
        assert!(parse_sse_data_line("data: {invalid}").is_none());
    }

    // === UsageMetadata ===

    #[test]
    fn test_usage_metadata_input_tokens() {
        let mut usage = UsageMetadata::default();
        usage.prompt_token_count = 1000;
        usage.cached_content_token_count = 200;
        assert_eq!(usage.input_tokens(), 800);
    }

    #[test]
    fn test_usage_metadata_output_tokens() {
        let mut usage = UsageMetadata::default();
        usage.candidates_token_count = 500;
        assert_eq!(usage.output_tokens(), 500);
    }

    #[test]
    fn test_usage_metadata_update_from() {
        let mut usage = UsageMetadata::default();
        let data = json!({
            "promptTokenCount": 1000,
            "candidatesTokenCount": 500,
            "cachedContentTokenCount": 200
        });
        usage.update_from(&data);
        assert_eq!(usage.prompt_token_count, 1000);
        assert_eq!(usage.candidates_token_count, 500);
        assert_eq!(usage.cached_content_token_count, 200);
    }

    #[test]
    fn test_usage_metadata_partial_update() {
        let mut usage = UsageMetadata::default();
        usage.prompt_token_count = 100;
        let data = json!({ "candidatesTokenCount": 50 });
        usage.update_from(&data);
        assert_eq!(usage.prompt_token_count, 100); // unchanged
        assert_eq!(usage.candidates_token_count, 50);
    }

    // === map_finish_reason ===

    #[test]
    fn test_finish_reason_stop() {
        assert_eq!(map_finish_reason(Some("STOP"), false), "end_turn");
    }

    #[test]
    fn test_finish_reason_max_tokens() {
        assert_eq!(map_finish_reason(Some("MAX_TOKENS"), false), "max_tokens");
    }

    #[test]
    fn test_finish_reason_tool_use() {
        assert_eq!(map_finish_reason(Some("TOOL_USE"), false), "tool_use");
    }

    #[test]
    fn test_finish_reason_has_tool_calls_overrides() {
        assert_eq!(map_finish_reason(Some("STOP"), true), "tool_use");
    }

    #[test]
    fn test_finish_reason_none() {
        assert_eq!(map_finish_reason(None, false), "end_turn");
    }

    // === generate_message_id / generate_tool_id ===

    #[test]
    fn test_generate_message_id_format() {
        let id = generate_message_id();
        assert!(id.starts_with("msg_"));
        assert!(id.len() > 4);
    }

    #[test]
    fn test_generate_tool_id_format() {
        let id = generate_tool_id();
        assert!(id.starts_with("toolu_"));
        assert!(id.len() > 6);
    }

    // === CloudCodeSseParser: text streaming ===

    #[test]
    fn test_stream_simple_text() {
        let mut parser = CloudCodeSseParser::new("claude-sonnet-4-5");
        let line = make_sse_chunk(
            vec![json!({ "text": "Hello world" })],
            Some("STOP"),
            Some(json!({ "promptTokenCount": 100, "candidatesTokenCount": 10 })),
        );

        let events = parser.process_line(&line);
        // Should have: message_start, content_block_start, content_block_delta
        assert!(events.len() >= 3);
        assert!(matches!(events[0], AnthropicSseEvent::MessageStart(_)));
        assert!(matches!(
            events[1],
            AnthropicSseEvent::ContentBlockStart { index: 0, .. }
        ));
        assert!(matches!(
            events[2],
            AnthropicSseEvent::ContentBlockDelta { index: 0, .. }
        ));

        let finish = parser.finish();
        // content_block_stop, message_delta, message_stop
        assert_eq!(finish.len(), 3);
        assert!(matches!(
            finish[0],
            AnthropicSseEvent::ContentBlockStop { index: 0 }
        ));
        assert!(matches!(finish[1], AnthropicSseEvent::MessageDelta(_)));
        assert!(matches!(finish[2], AnthropicSseEvent::MessageStop));
    }

    #[test]
    fn test_stream_empty_text_skipped() {
        let mut parser = CloudCodeSseParser::new("test-model");
        let line = make_sse_chunk(vec![json!({ "text": "" })], None, None);
        let events = parser.process_line(&line);
        // message_start only (empty text is skipped)
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], AnthropicSseEvent::MessageStart(_)));
    }

    // === CloudCodeSseParser: thinking blocks ===

    #[test]
    fn test_stream_thinking_then_text() {
        let mut parser = CloudCodeSseParser::new("gemini-3-flash");
        let sig = valid_signature();

        // Thinking chunk
        let line1 = make_sse_chunk(
            vec![json!({ "text": "Let me think...", "thought": true, "thoughtSignature": sig })],
            None,
            None,
        );
        let events1 = parser.process_line(&line1);
        assert!(events1.len() >= 3); // message_start, block_start, thinking_delta

        // Text chunk
        let line2 = make_sse_chunk(
            vec![json!({ "text": "Here is the answer" })],
            Some("STOP"),
            None,
        );
        let events2 = parser.process_line(&line2);
        // signature_delta, block_stop, block_start, text_delta
        assert!(events2.len() >= 4);

        // Verify signature_delta was emitted
        let has_sig_delta = events2.iter().any(|e| {
            if let AnthropicSseEvent::ContentBlockDelta { delta, .. } = e {
                delta.get("type").and_then(|v| v.as_str()) == Some("signature_delta")
            } else {
                false
            }
        });
        assert!(
            has_sig_delta,
            "Should emit signature_delta before text block"
        );
    }

    #[test]
    fn test_stream_thinking_signature_at_finish() {
        let mut parser = CloudCodeSseParser::new("gemini-3-flash");
        let sig = valid_signature();

        let line = make_sse_chunk(
            vec![json!({ "text": "thinking...", "thought": true, "thoughtSignature": sig })],
            Some("STOP"),
            None,
        );
        parser.process_line(&line);

        let finish = parser.finish();
        // Should have: signature_delta, block_stop, message_delta, message_stop
        let has_sig = finish.iter().any(|e| {
            if let AnthropicSseEvent::ContentBlockDelta { delta, .. } = e {
                delta.get("type").and_then(|v| v.as_str()) == Some("signature_delta")
            } else {
                false
            }
        });
        assert!(has_sig, "Should emit signature_delta at finish");
    }

    // === CloudCodeSseParser: tool use ===

    #[test]
    fn test_stream_function_call() {
        let mut parser = CloudCodeSseParser::new("claude-sonnet-4-5");
        let line = make_sse_chunk(
            vec![json!({
                "functionCall": {
                    "name": "read_file",
                    "args": { "path": "/tmp/test.txt" },
                    "id": "call_123"
                }
            })],
            None,
            None,
        );

        let events = parser.process_line(&line);
        // message_start, content_block_start (tool_use), content_block_delta (input_json)
        assert!(events.len() >= 3);

        // Verify tool_use block
        if let AnthropicSseEvent::ContentBlockStart { content_block, .. } = &events[1] {
            assert_eq!(content_block["type"], "tool_use");
            assert_eq!(content_block["name"], "read_file");
            assert_eq!(content_block["id"], "call_123");
        } else {
            panic!("Expected ContentBlockStart");
        }

        // Verify input_json_delta
        if let AnthropicSseEvent::ContentBlockDelta { delta, .. } = &events[2] {
            assert_eq!(delta["type"], "input_json_delta");
            let partial: Value =
                serde_json::from_str(delta["partial_json"].as_str().unwrap()).unwrap();
            assert_eq!(partial["path"], "/tmp/test.txt");
        } else {
            panic!("Expected ContentBlockDelta");
        }

        // has_tool_calls should set stop_reason to tool_use
        let finish = parser.finish();
        if let AnthropicSseEvent::MessageDelta(delta) = &finish[1] {
            assert_eq!(delta["delta"]["stop_reason"], "tool_use");
        }
    }

    #[test]
    fn test_stream_function_call_with_thought_signature() {
        let mut parser = CloudCodeSseParser::new("gemini-3-flash");
        let sig = valid_signature();
        let line = make_sse_chunk(
            vec![json!({
                "functionCall": { "name": "bash", "args": {} },
                "thoughtSignature": sig
            })],
            None,
            None,
        );

        let events = parser.process_line(&line);
        if let AnthropicSseEvent::ContentBlockStart { content_block, .. } = &events[1] {
            assert_eq!(content_block["thoughtSignature"], sig);
        } else {
            panic!("Expected ContentBlockStart with thoughtSignature");
        }
    }

    // === CloudCodeSseParser: inline data (image) ===

    #[test]
    fn test_stream_inline_data() {
        let mut parser = CloudCodeSseParser::new("gemini-3-flash");
        let line = make_sse_chunk(
            vec![json!({
                "inlineData": {
                    "mimeType": "image/png",
                    "data": "iVBORw0KGgo="
                }
            })],
            None,
            None,
        );

        let events = parser.process_line(&line);
        // message_start, content_block_start (image), content_block_stop
        assert!(events.len() >= 3);

        if let AnthropicSseEvent::ContentBlockStart { content_block, .. } = &events[1] {
            assert_eq!(content_block["type"], "image");
            assert_eq!(content_block["source"]["media_type"], "image/png");
            assert_eq!(content_block["source"]["data"], "iVBORw0KGgo=");
        } else {
            panic!("Expected ContentBlockStart for image");
        }
    }

    // === CloudCodeSseParser: no content ===

    #[test]
    fn test_stream_no_content_finish() {
        let mut parser = CloudCodeSseParser::new("test-model");
        assert!(!parser.has_content());
        let finish = parser.finish();
        assert!(finish.is_empty());
    }

    // === CloudCodeSseParser: multi-chunk streaming ===

    #[test]
    fn test_stream_multiple_text_chunks() {
        let mut parser = CloudCodeSseParser::new("test-model");

        let line1 = make_sse_chunk(vec![json!({ "text": "Hello " })], None, None);
        let line2 = make_sse_chunk(vec![json!({ "text": "world" })], Some("STOP"), None);

        let events1 = parser.process_line(&line1);
        let events2 = parser.process_line(&line2);

        // First chunk: message_start + block_start + delta
        assert_eq!(events1.len(), 3);
        // Second chunk: just delta (same block type)
        assert_eq!(events2.len(), 1);
        assert!(matches!(
            events2[0],
            AnthropicSseEvent::ContentBlockDelta { index: 0, .. }
        ));
    }

    // === CloudCodeSseParser: usage in message_start ===

    #[test]
    fn test_stream_usage_in_message_start() {
        let mut parser = CloudCodeSseParser::new("test-model");
        let line = make_sse_chunk(
            vec![json!({ "text": "hi" })],
            None,
            Some(json!({
                "promptTokenCount": 1000,
                "candidatesTokenCount": 50,
                "cachedContentTokenCount": 200
            })),
        );

        let events = parser.process_line(&line);
        if let AnthropicSseEvent::MessageStart(msg) = &events[0] {
            assert_eq!(msg["usage"]["input_tokens"], 800); // 1000 - 200
            assert_eq!(msg["usage"]["cache_read_input_tokens"], 200);
        } else {
            panic!("Expected MessageStart");
        }
    }

    // === AnthropicSseEvent::to_sse_string ===

    #[test]
    fn test_event_to_sse_string_message_stop() {
        let event = AnthropicSseEvent::MessageStop;
        let sse = event.to_sse_string();
        assert!(sse.starts_with("event: message_stop\n"));
        assert!(sse.contains("\"type\":\"message_stop\""));
    }

    #[test]
    fn test_event_to_sse_string_content_block_delta() {
        let event = AnthropicSseEvent::ContentBlockDelta {
            index: 0,
            delta: json!({ "type": "text_delta", "text": "hello" }),
        };
        let sse = event.to_sse_string();
        assert!(sse.starts_with("event: content_block_delta\n"));
        assert!(sse.contains("\"index\":0"));
    }

    // === parse_collected_response ===

    #[test]
    fn test_collected_simple_text() {
        let line = make_sse_chunk(
            vec![json!({ "text": "Hello world" })],
            Some("STOP"),
            Some(json!({ "promptTokenCount": 100, "candidatesTokenCount": 10 })),
        );

        let result = parse_collected_response(&line, "test-model");
        assert_eq!(result["type"], "message");
        assert_eq!(result["role"], "assistant");
        assert_eq!(result["model"], "test-model");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello world");
        assert_eq!(result["usage"]["input_tokens"], 100);
        assert_eq!(result["usage"]["output_tokens"], 10);
    }

    #[test]
    fn test_collected_thinking_and_text() {
        let sig = valid_signature();
        let lines = format!(
            "{}\n{}",
            make_sse_chunk(
                vec![json!({ "text": "Let me think", "thought": true, "thoughtSignature": sig })],
                None,
                None,
            ),
            make_sse_chunk(
                vec![json!({ "text": "The answer is 42" })],
                Some("STOP"),
                Some(json!({ "promptTokenCount": 200, "candidatesTokenCount": 50 })),
            ),
        );

        let result = parse_collected_response(&lines, "gemini-3-flash");
        assert_eq!(result["content"].as_array().unwrap().len(), 2);
        assert_eq!(result["content"][0]["type"], "thinking");
        assert_eq!(result["content"][0]["thinking"], "Let me think");
        assert_eq!(result["content"][0]["signature"], sig);
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "The answer is 42");
    }

    #[test]
    fn test_collected_function_call() {
        let line = make_sse_chunk(
            vec![json!({
                "functionCall": {
                    "name": "read_file",
                    "args": { "path": "/tmp" },
                    "id": "call_abc"
                }
            })],
            Some("STOP"),
            None,
        );

        let result = parse_collected_response(&line, "claude-sonnet-4-5");
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "read_file");
        assert_eq!(result["content"][0]["id"], "call_abc");
        assert_eq!(result["content"][0]["input"]["path"], "/tmp");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_collected_inline_data() {
        let line = make_sse_chunk(
            vec![json!({
                "inlineData": { "mimeType": "image/png", "data": "abc123" }
            })],
            Some("STOP"),
            None,
        );

        let result = parse_collected_response(&line, "gemini-3-flash");
        assert_eq!(result["content"][0]["type"], "image");
        assert_eq!(result["content"][0]["source"]["media_type"], "image/png");
        assert_eq!(result["content"][0]["source"]["data"], "abc123");
    }

    #[test]
    fn test_collected_empty_response() {
        let result = parse_collected_response("", "test-model");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "");
    }

    #[test]
    fn test_collected_usage_cache_adjustment() {
        let line = make_sse_chunk(
            vec![json!({ "text": "hi" })],
            Some("STOP"),
            Some(json!({
                "promptTokenCount": 1000,
                "candidatesTokenCount": 50,
                "cachedContentTokenCount": 300
            })),
        );

        let result = parse_collected_response(&line, "test-model");
        assert_eq!(result["usage"]["input_tokens"], 700); // 1000 - 300
        assert_eq!(result["usage"]["output_tokens"], 50);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 300);
    }

    #[test]
    fn test_collected_max_tokens_stop_reason() {
        let line = make_sse_chunk(
            vec![json!({ "text": "truncated" })],
            Some("MAX_TOKENS"),
            None,
        );

        let result = parse_collected_response(&line, "test-model");
        assert_eq!(result["stop_reason"], "max_tokens");
    }

    #[test]
    fn test_collected_accumulated_thinking_across_chunks() {
        let sig = valid_signature();
        let lines = format!(
            "{}\n{}",
            make_sse_chunk(
                vec![json!({ "text": "Part 1 ", "thought": true })],
                None,
                None,
            ),
            make_sse_chunk(
                vec![json!({ "text": "Part 2", "thought": true, "thoughtSignature": sig })],
                Some("STOP"),
                None,
            ),
        );

        let result = parse_collected_response(&lines, "gemini-3-flash");
        assert_eq!(result["content"][0]["type"], "thinking");
        assert_eq!(result["content"][0]["thinking"], "Part 1 Part 2");
        assert_eq!(result["content"][0]["signature"], sig);
    }

    #[test]
    fn test_collected_mixed_content() {
        let sig = valid_signature();
        let lines = format!(
            "{}\n{}\n{}",
            make_sse_chunk(
                vec![json!({ "text": "thinking...", "thought": true, "thoughtSignature": sig })],
                None,
                None,
            ),
            make_sse_chunk(vec![json!({ "text": "Here is the result" })], None, None,),
            make_sse_chunk(
                vec![json!({
                    "functionCall": { "name": "bash", "args": { "cmd": "ls" }, "id": "t1" }
                })],
                Some("STOP"),
                Some(json!({ "promptTokenCount": 500, "candidatesTokenCount": 100 })),
            ),
        );

        let result = parse_collected_response(&lines, "gemini-3-flash");
        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert_eq!(content[0]["type"], "thinking");
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "Here is the result");
        assert_eq!(content[2]["type"], "tool_use");
        assert_eq!(content[2]["name"], "bash");
        assert_eq!(result["stop_reason"], "tool_use");
    }
}
