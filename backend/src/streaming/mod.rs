pub mod cross_format;
pub mod sse;

use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{timeout, Duration};
use tracing::warn;

use crate::error::ApiError;
use crate::middleware::DEBUG_LOGGER;
use crate::thinking_parser::ThinkingParser;

// ==================================================================================================
// Data Structures
// ==================================================================================================

/// Unified event from Kiro API stream.
///
/// This format is API-agnostic and can be converted to both OpenAI and Anthropic formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroEvent {
    /// Event type (content, thinking, tool_use, usage, context_usage, error)
    #[serde(rename = "type")]
    pub event_type: String,

    /// Text content (for content events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Thinking/reasoning content (for thinking events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_content: Option<String>,

    /// Tool use data (for tool_use events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<ToolUse>,

    /// Usage/metering data (for usage events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,

    /// Context usage percentage (for context_usage events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_usage_percentage: Option<f64>,

    /// Whether this is the first thinking chunk
    #[serde(default)]
    pub is_first_thinking_chunk: bool,

    /// Whether this is the last thinking chunk
    #[serde(default)]
    pub is_last_thinking_chunk: bool,
}

/// Tool use information from Kiro API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub tool_use_id: String,
    pub name: String,
    pub input: Value,
    /// Truncation info set when JSON parsing fails due to truncation (not serialized)
    #[serde(skip)]
    pub truncation_info: Option<crate::truncation::TruncationInfo>,
}

/// Usage information from Kiro API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// Result of collecting a complete stream response
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct StreamResult {
    pub content: String,
    pub thinking_content: String,
    pub tool_calls: Vec<ToolUse>,
    pub usage: Option<Usage>,
    pub context_usage_percentage: Option<f64>,
}

// ==================================================================================================
// Tool Call Deduplication
// ==================================================================================================

/// Removes duplicate tool calls.
///
/// Deduplication occurs by two criteria:
/// 1. By id - if there are multiple tool calls with the same id, keep the one with
///    more arguments (not empty "{}")
/// 2. By name+arguments - remove complete duplicates
pub fn deduplicate_tool_calls(tool_calls: Vec<ToolUse>) -> Vec<ToolUse> {
    use std::collections::{HashMap, HashSet};

    if tool_calls.is_empty() {
        return tool_calls;
    }

    // First deduplicate by id - keep tool call with non-empty arguments
    let mut by_id: HashMap<String, ToolUse> = HashMap::new();
    let mut without_id: Vec<ToolUse> = Vec::new();

    for tc in tool_calls.iter() {
        if tc.tool_use_id.is_empty() {
            without_id.push(tc.clone());
            continue;
        }

        if let Some(existing) = by_id.get(&tc.tool_use_id) {
            // Duplicate by id exists - keep the one with more arguments
            let existing_args = serde_json::to_string(&existing.input).unwrap_or_default();
            let current_args = serde_json::to_string(&tc.input).unwrap_or_default();

            // Prefer non-empty arguments
            if current_args != "{}"
                && (existing_args == "{}" || current_args.len() > existing_args.len())
            {
                tracing::debug!(
                    "Replacing tool call {} with better arguments: {} -> {}",
                    tc.tool_use_id,
                    existing_args.len(),
                    current_args.len()
                );
                by_id.insert(tc.tool_use_id.clone(), tc.clone());
            }
        } else {
            by_id.insert(tc.tool_use_id.clone(), tc.clone());
        }
    }

    // Collect tool calls: first those with id, then without id
    let result_with_id: Vec<ToolUse> = by_id.into_values().collect();

    // Now deduplicate by name+arguments for all
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: Vec<ToolUse> = Vec::new();

    for tc in result_with_id.into_iter().chain(without_id.into_iter()) {
        let args_str = serde_json::to_string(&tc.input).unwrap_or_default();
        let key = format!("{}-{}", tc.name, args_str);

        if !seen.contains(&key) {
            seen.insert(key);
            unique.push(tc);
        }
    }

    if tool_calls.len() != unique.len() {
        tracing::debug!(
            "Deduplicated tool calls: {} -> {}",
            tool_calls.len(),
            unique.len()
        );
    }

    unique
}

// ==================================================================================================
// SSE Parsing
// ==================================================================================================

/// Accumulates tool call data across multiple stream events.
///
/// Kiro API sends tool calls in multiple events:
/// 1. {"name": "...", "toolUseId": "..."} - tool start
/// 2. {"input": "...", "name": "...", "toolUseId": "..."} - input chunks (multiple)
/// 3. {"stop": true} - tool end
#[derive(Debug, Clone, Default)]
pub struct ToolCallAccumulator {
    /// Current tool being accumulated
    current_tool: Option<AccumulatingTool>,
    /// Completed tool calls
    pub completed_tools: Vec<ToolUse>,
    /// Whether finalize() has been called (prevents double-finalization on stream errors)
    finalized: bool,
}

#[derive(Debug, Clone)]
struct AccumulatingTool {
    tool_use_id: String,
    name: String,
    input_str: String,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a tool-related event and return a completed tool if ready
    ///
    /// Kiro API sends tool events in this format:
    /// - First event: {"name": "Bash", "toolUseId": "xxx", "input": "..."}
    /// - Continuation events: {"name": "Bash", "toolUseId": "xxx", "input": "..."} (same toolUseId!)
    /// - Stop event: {"stop": true}
    ///
    /// IMPORTANT: Kiro sends `name` and `toolUseId` in EVERY input chunk, not just the first one.
    /// We must check if it's the SAME tool (same toolUseId) and append input, not start a new tool.
    pub fn process_event(&mut self, json: &Value) -> Option<ToolUse> {
        tracing::debug!("ToolCallAccumulator::process_event: {}", json);

        // Extract toolUseId if present
        let event_tool_use_id = json
            .get("toolUseId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Check if this is a continuation of the current tool (same toolUseId)
        let is_same_tool = if let (Some(ref current), Some(ref event_id)) =
            (&self.current_tool, &event_tool_use_id)
        {
            !current.tool_use_id.is_empty() && current.tool_use_id == *event_id
        } else {
            false
        };

        // If we have name field, it could be a new tool start OR a continuation with name repeated
        if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
            if is_same_tool {
                // Same toolUseId - this is a continuation, just append input
                tracing::debug!("Tool input continuation (same toolUseId): name={}", name);
                if let Some(ref mut tool) = self.current_tool {
                    if let Some(input) = json.get("input") {
                        match input {
                            Value::String(s) => {
                                tracing::debug!(
                                    "Appending input string: {} chars, total now: {}",
                                    s.len(),
                                    tool.input_str.len() + s.len()
                                );
                                tool.input_str.push_str(s);
                            }
                            v => {
                                let s = serde_json::to_string(v).unwrap_or_default();
                                tracing::debug!("Appending input json: {} chars", s.len());
                                tool.input_str.push_str(&s);
                            }
                        }
                    }
                }

                // Check if this event also has stop
                if json.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
                    tracing::debug!("Continuation event has stop=true, finalizing");
                    return self.finalize_current();
                }

                return None;
            } else {
                // Different toolUseId or no current tool - this is a NEW tool start
                tracing::debug!(
                    "Tool start detected: name={}, toolUseId={:?}",
                    name,
                    event_tool_use_id
                );

                // Finalize previous tool if exists
                let completed = self.finalize_current();

                let tool_use_id = event_tool_use_id.unwrap_or_default();

                // Get initial input if present
                let input_str = match json.get("input") {
                    Some(Value::String(s)) => {
                        tracing::debug!("Initial input (string): {} chars", s.len());
                        s.clone()
                    }
                    Some(v) => {
                        let s = serde_json::to_string(v).unwrap_or_default();
                        tracing::debug!("Initial input (json): {} chars", s.len());
                        s
                    }
                    None => {
                        tracing::debug!("No initial input");
                        String::new()
                    }
                };

                self.current_tool = Some(AccumulatingTool {
                    tool_use_id,
                    name: name.to_string(),
                    input_str,
                });

                // Check if this event also has stop
                if json.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
                    tracing::debug!("Tool has stop=true, finalizing immediately");
                    return self.finalize_current();
                }

                return completed;
            }
        }

        // Tool input continuation without name: {"input": "...", ...}
        if let Some(input) = json.get("input") {
            tracing::debug!("Tool input continuation detected (no name field)");
            if let Some(ref mut tool) = self.current_tool {
                match input {
                    Value::String(s) => {
                        tracing::debug!(
                            "Appending input string: {} chars, total now: {}",
                            s.len(),
                            tool.input_str.len() + s.len()
                        );
                        tool.input_str.push_str(s);
                    }
                    v => {
                        let s = serde_json::to_string(v).unwrap_or_default();
                        tracing::debug!("Appending input json: {} chars", s.len());
                        tool.input_str.push_str(&s);
                    }
                }
            } else {
                tracing::warn!("Got input event but no current tool!");
            }

            // Check if this event also has stop
            if json.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
                tracing::debug!("Input event has stop=true, finalizing");
                return self.finalize_current();
            }
        }

        // Tool stop: {"stop": true}
        if json.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::debug!("Tool stop detected");
            return self.finalize_current();
        }

        None
    }

    /// Finalize current tool and return it
    fn finalize_current(&mut self) -> Option<ToolUse> {
        let tool = self.current_tool.take()?;

        tracing::debug!(
            "Finalizing tool '{}' with raw arguments: {}",
            tool.name,
            if tool.input_str.len() > 200 {
                let end = tool.input_str.floor_char_boundary(200);
                format!("{}...", &tool.input_str[..end])
            } else {
                tool.input_str.clone()
            }
        );

        let mut truncation_info = None;

        // Parse accumulated input string as JSON
        let input = if tool.input_str.is_empty() {
            tracing::debug!(
                "Tool '{}' has empty arguments string (will be deduplicated)",
                tool.name
            );
            Value::Object(Default::default())
        } else {
            match serde_json::from_str::<Value>(&tool.input_str) {
                Ok(parsed) => {
                    if let Some(obj) = parsed.as_object() {
                        tracing::debug!(
                            "Tool '{}' arguments parsed successfully: {:?}",
                            tool.name,
                            obj.keys().collect::<Vec<_>>()
                        );
                    }
                    parsed
                }
                Err(e) => {
                    // Diagnose if this is truncation vs malformed JSON
                    let info = crate::truncation::diagnose_json_truncation(&tool.input_str);
                    if info.is_truncated {
                        tracing::error!(
                            "TRUNCATION DETECTED: tool '{}' (id={}) arguments truncated: {} (raw size: {} bytes)",
                            tool.name,
                            tool.tool_use_id,
                            info.reason,
                            info.size_bytes
                        );
                        truncation_info = Some(info);
                    } else {
                        tracing::warn!(
                            "Failed to parse tool '{}' arguments: {}. Raw: {}",
                            tool.name,
                            e,
                            if tool.input_str.len() > 200 {
                                let end = tool.input_str.floor_char_boundary(200);
                                format!("{}...", &tool.input_str[..end])
                            } else {
                                tool.input_str.clone()
                            }
                        );
                    }
                    Value::Object(Default::default())
                }
            }
        };

        let completed = ToolUse {
            tool_use_id: tool.tool_use_id,
            name: tool.name,
            input,
            truncation_info,
        };

        self.completed_tools.push(completed.clone());
        Some(completed)
    }

    /// Finalize any remaining tool at end of stream.
    /// Returns None if already finalized (prevents double-finalization on stream errors).
    pub fn finalize(&mut self) -> Option<ToolUse> {
        if self.finalized {
            return None;
        }
        self.finalized = true;
        self.finalize_current()
    }
}

/// Parses AWS Event Stream format from Kiro API.
///
/// The Kiro API returns events in AWS Event Stream binary format where
/// JSON events are embedded directly in the stream. This parser extracts
/// JSON objects by looking for known patterns and using brace matching.
///
/// Supported event patterns:
/// - {"content": "..."} - Text content
/// - {"name": "...", "toolUseId": "...", "input": ...} - Tool start
/// - {"input": "..."} - Tool input continuation
/// - {"stop": true} - Tool stop
/// - {"usage": ...} - Usage information
/// - {"contextUsagePercentage": ...} - Context usage
pub struct SseParser {
    buffer: String,
    pub tool_accumulator: ToolCallAccumulator,
}

/// Patterns to look for in the stream (in order of priority)
const EVENT_PATTERNS: &[&str] = &[
    "{\"content\":",
    "{\"name\":",
    "{\"input\":",
    "{\"stop\":",
    "{\"followupPrompt\":",
    "{\"usage\":",
    "{\"contextUsagePercentage\":",
];

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            tool_accumulator: ToolCallAccumulator::new(),
        }
    }

    /// Feed bytes into the parser and extract complete events.
    ///
    /// Returns a vector of JSON events.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Value>, ApiError> {
        // Convert bytes to string, ignoring invalid UTF-8
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);

        let mut events = Vec::new();

        // Keep extracting events while we can find patterns
        loop {
            // Find the earliest pattern in the buffer
            let mut earliest_pos: Option<usize> = None;

            for pattern in EVENT_PATTERNS {
                if let Some(pos) = self.buffer.find(pattern) {
                    if earliest_pos.is_none() || pos < earliest_pos.unwrap() {
                        earliest_pos = Some(pos);
                    }
                }
            }

            let Some(json_start) = earliest_pos else {
                break;
            };

            // Find the matching closing brace
            let Some(json_end) = find_matching_brace(&self.buffer, json_start) else {
                // JSON not complete yet, wait for more data
                break;
            };

            // Extract the JSON string
            let json_str = &self.buffer[json_start..=json_end];

            // Try to parse it
            match serde_json::from_str::<Value>(json_str) {
                Ok(json) => {
                    events.push(json);
                }
                Err(e) => {
                    warn!(
                        "Failed to parse JSON: {} - {}",
                        e,
                        &json_str[..json_str.len().min(100)]
                    );
                }
            }

            // Remove the processed part from buffer
            self.buffer = self.buffer[json_end + 1..].to_string();
        }

        Ok(events)
    }

    /// Finalize parsing and return any remaining buffered data.
    #[allow(dead_code)]
    pub fn finalize(&mut self) -> Result<Vec<Value>, ApiError> {
        if self.buffer.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Try to extract any remaining events
        self.feed(&[])
    }
}

/// Finds the position of the closing brace considering nesting and strings.
///
/// Uses bracket counting for correct parsing of nested JSON.
/// Accounts for quoted strings and escape sequences.
///
/// IMPORTANT: This function handles AWS Event Stream format where JSON payloads
/// are followed by binary framing data. The binary data may contain bytes that
/// decode to quote characters when using lossy UTF-8 conversion, so we must
/// return as soon as we find the matching brace.
#[allow(clippy::needless_range_loop)]
fn find_matching_brace(text: &str, start_pos: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    if start_pos >= bytes.len() || bytes[start_pos] != b'{' {
        return None;
    }

    let mut brace_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for i in start_pos..bytes.len() {
        let ch = bytes[i];

        if escape_next {
            escape_next = false;
            continue;
        }

        if ch == b'\\' && in_string {
            escape_next = true;
            continue;
        }

        if ch == b'"' {
            in_string = !in_string;
            continue;
        }

        if !in_string {
            if ch == b'{' {
                brace_count += 1;
            } else if ch == b'}' {
                brace_count -= 1;
                if brace_count == 0 {
                    return Some(i);
                }
            }
        }
    }

    None
}

// ==================================================================================================
// Kiro Event Parsing
// ==================================================================================================

/// Converts Kiro API JSON events to KiroEvent objects.
///
/// Tool events are processed through the accumulator to properly combine
/// streamed tool input chunks into complete tool calls.
pub fn parse_kiro_event_with_accumulator(
    json: &Value,
    tool_acc: &mut ToolCallAccumulator,
) -> Option<KiroEvent> {
    // Skip followupPrompt events
    if json.get("followupPrompt").is_some() {
        return None;
    }

    // Content event: {"content": "Hello"}
    if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
        return Some(KiroEvent {
            event_type: "content".to_string(),
            content: Some(content.to_string()),
            thinking_content: None,
            tool_use: None,
            usage: None,
            context_usage_percentage: None,
            is_first_thinking_chunk: false,
            is_last_thinking_chunk: false,
        });
    }

    // Tool events: {"name": ...}, {"input": ...}, {"stop": true}
    // These are processed through the accumulator
    if json.get("name").is_some() || json.get("input").is_some() || json.get("stop").is_some() {
        tracing::info!(
            "Tool event: name={}, input_len={}, stop={}",
            json.get("name").and_then(|v| v.as_str()).unwrap_or("-"),
            json.get("input")
                .map(|v| v.as_str().map(|s| s.len()).unwrap_or(0))
                .unwrap_or(0),
            json.get("stop")
                .and_then(|v| v.as_bool())
                .map(|b| b.to_string())
                .unwrap_or("-".to_string())
        );
        if let Some(completed_tool) = tool_acc.process_event(json) {
            return Some(KiroEvent {
                event_type: "tool_use".to_string(),
                content: None,
                thinking_content: None,
                tool_use: Some(completed_tool),
                usage: None,
                context_usage_percentage: None,
                is_first_thinking_chunk: false,
                is_last_thinking_chunk: false,
            });
        }
        // Tool is still accumulating, no event to emit yet
        return None;
    }

    // Usage event: {"usage": 1.5}
    if let Some(usage_val) = json.get("usage") {
        // Usage can be a number (credits) or an object with inputTokens/outputTokens
        if let Some(usage_num) = usage_val.as_f64() {
            // Simple usage number - convert to tokens (approximate)
            return Some(KiroEvent {
                event_type: "usage".to_string(),
                content: None,
                thinking_content: None,
                tool_use: None,
                usage: Some(Usage {
                    input_tokens: 0,
                    output_tokens: (usage_num * 1000.0) as i32, // Approximate conversion
                }),
                context_usage_percentage: None,
                is_first_thinking_chunk: false,
                is_last_thinking_chunk: false,
            });
        } else if usage_val.is_object() {
            let input_tokens = usage_val
                .get("inputTokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let output_tokens = usage_val
                .get("outputTokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;

            return Some(KiroEvent {
                event_type: "usage".to_string(),
                content: None,
                thinking_content: None,
                tool_use: None,
                usage: Some(Usage {
                    input_tokens,
                    output_tokens,
                }),
                context_usage_percentage: None,
                is_first_thinking_chunk: false,
                is_last_thinking_chunk: false,
            });
        }
    }

    // Context usage event: {"contextUsagePercentage": 50.0}
    if let Some(ctx_usage) = json.get("contextUsagePercentage").and_then(|v| v.as_f64()) {
        return Some(KiroEvent {
            event_type: "context_usage".to_string(),
            content: None,
            thinking_content: None,
            tool_use: None,
            usage: None,
            context_usage_percentage: Some(ctx_usage),
            is_first_thinking_chunk: false,
            is_last_thinking_chunk: false,
        });
    }

    // Legacy format support: {"contentBlockDelta": {"delta": {"text": "Hello"}}}
    if let Some(content_block_delta) = json.get("contentBlockDelta") {
        if let Some(delta) = content_block_delta.get("delta") {
            // Text content
            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                return Some(KiroEvent {
                    event_type: "content".to_string(),
                    content: Some(text.to_string()),
                    thinking_content: None,
                    tool_use: None,
                    usage: None,
                    context_usage_percentage: None,
                    is_first_thinking_chunk: false,
                    is_last_thinking_chunk: false,
                });
            }

            // Tool use (legacy format - not streamed)
            if let Some(tool_use) = delta.get("toolUse") {
                let tool_use_id = tool_use
                    .get("toolUseId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = tool_use
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = tool_use
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default()));

                return Some(KiroEvent {
                    event_type: "tool_use".to_string(),
                    content: None,
                    thinking_content: None,
                    tool_use: Some(ToolUse {
                        tool_use_id,
                        name,
                        input,
                        truncation_info: None,
                    }),
                    usage: None,
                    context_usage_percentage: None,
                    is_first_thinking_chunk: false,
                    is_last_thinking_chunk: false,
                });
            }
        }
    }

    // Legacy usage metadata: {"metadata": {"usage": {...}}}
    if let Some(metadata) = json.get("metadata") {
        if let Some(usage) = metadata.get("usage") {
            let input_tokens = usage
                .get("inputTokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let output_tokens = usage
                .get("outputTokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;

            return Some(KiroEvent {
                event_type: "usage".to_string(),
                content: None,
                thinking_content: None,
                tool_use: None,
                usage: Some(Usage {
                    input_tokens,
                    output_tokens,
                }),
                context_usage_percentage: None,
                is_first_thinking_chunk: false,
                is_last_thinking_chunk: false,
            });
        }
    }

    // Message stop
    if json.get("messageStop").is_some() {
        return None;
    }

    None
}

/// Backward-compatible version for tests - creates a temporary accumulator
#[cfg(test)]
pub fn parse_kiro_event(json: &Value) -> Option<KiroEvent> {
    let mut acc = ToolCallAccumulator::new();
    parse_kiro_event_with_accumulator(json, &mut acc)
}

// ==================================================================================================
// Stream Utilities
// ==================================================================================================

/// Parse a Kiro SSE stream into KiroEvent objects.
///
/// This is the main entry point for parsing Kiro API streams.
/// It uses ThinkingParser to detect and extract <thinking> blocks from content.
pub async fn parse_kiro_stream(
    response: reqwest::Response,
    first_token_timeout_secs: u64,
) -> Result<impl Stream<Item = Result<KiroEvent, ApiError>>, ApiError> {
    parse_kiro_stream_with_thinking(response, first_token_timeout_secs, true).await
}

/// Parse a Kiro SSE stream with optional thinking parser.
pub async fn parse_kiro_stream_with_thinking(
    response: reqwest::Response,
    first_token_timeout_secs: u64,
    enable_thinking_parser: bool,
) -> Result<impl Stream<Item = Result<KiroEvent, ApiError>>, ApiError> {
    let mut byte_stream = response.bytes_stream();
    let mut parser = SseParser::new();

    // Wait for first chunk with timeout
    let first_chunk = timeout(
        Duration::from_secs(first_token_timeout_secs),
        byte_stream.next(),
    )
    .await
    .map_err(|_| {
        warn!(
            "[FirstTokenTimeout] Model did not respond within {}s",
            first_token_timeout_secs
        );
        ApiError::Internal(anyhow::anyhow!("First token timeout"))
    })?;

    let first_chunk = match first_chunk {
        Some(Ok(chunk)) => chunk,
        Some(Err(e)) => return Err(ApiError::Internal(anyhow::anyhow!("Stream error: {}", e))),
        None => {
            return Ok(futures::stream::empty().boxed());
        }
    };

    // Log raw chunk for debugging
    DEBUG_LOGGER.log_raw_chunk(first_chunk.clone()).await;

    // Initialize thinking parser if enabled
    let thinking_parser = if enable_thinking_parser {
        Some(std::sync::Arc::new(std::sync::Mutex::new(
            ThinkingParser::new(),
        )))
    } else {
        None
    };

    // Tool accumulator for combining streamed tool input
    let tool_accumulator = std::sync::Arc::new(std::sync::Mutex::new(ToolCallAccumulator::new()));

    // Process first chunk through thinking parser
    let mut events = Vec::new();
    let jsons = parser.feed(&first_chunk)?;

    for json in jsons {
        let mut tool_acc = tool_accumulator.lock().unwrap();
        if let Some(event) = parse_kiro_event_with_accumulator(&json, &mut tool_acc) {
            drop(tool_acc); // Release lock before processing
                            // Process content events through thinking parser
            if event.event_type == "content" {
                if let Some(content) = &event.content {
                    if let Some(ref tp) = thinking_parser {
                        let mut tp_guard = tp.lock().unwrap();
                        let parse_result = tp_guard.feed(content);

                        // Yield thinking content if any
                        if let Some(thinking) = parse_result.thinking_content {
                            let processed = tp_guard.process_for_output(
                                &thinking,
                                parse_result.is_first_thinking_chunk,
                                parse_result.is_last_thinking_chunk,
                            );
                            if let Some(processed_thinking) = processed {
                                events.push(Ok(KiroEvent {
                                    event_type: "thinking".to_string(),
                                    content: None,
                                    thinking_content: Some(processed_thinking),
                                    tool_use: None,
                                    usage: None,
                                    context_usage_percentage: None,
                                    is_first_thinking_chunk: parse_result.is_first_thinking_chunk,
                                    is_last_thinking_chunk: parse_result.is_last_thinking_chunk,
                                }));
                            }
                        }

                        // Yield regular content if any
                        if let Some(regular) = parse_result.regular_content {
                            events.push(Ok(KiroEvent {
                                event_type: "content".to_string(),
                                content: Some(regular),
                                thinking_content: None,
                                tool_use: None,
                                usage: None,
                                context_usage_percentage: None,
                                is_first_thinking_chunk: false,
                                is_last_thinking_chunk: false,
                            }));
                        }
                    } else {
                        // No thinking parser - pass through as-is
                        events.push(Ok(event));
                    }
                }
            } else {
                // Non-content events pass through
                events.push(Ok(event));
            }
        }
    }

    // Clone thinking parser and tool accumulator for use in remaining stream
    let thinking_parser_for_stream = thinking_parser.clone();
    let tool_accumulator_for_stream = tool_accumulator.clone();

    // Wrap parser in Arc<Mutex<>> to share state across chunks
    let parser = std::sync::Arc::new(std::sync::Mutex::new(parser));
    let parser_for_stream = parser.clone();

    // Create stream that yields first chunk events, then continues with remaining chunks
    let remaining_stream = byte_stream
        .then(move |chunk_result| {
            let parser = parser_for_stream.clone();
            let tp = thinking_parser_for_stream.clone();
            let tool_acc = tool_accumulator_for_stream.clone();

            async move {
                match chunk_result {
                    Ok(chunk) => {
                        // Log raw chunk for debugging
                        DEBUG_LOGGER.log_raw_chunk(chunk.clone()).await;

                        let mut events = Vec::new();
                        let mut parser_guard = parser.lock().unwrap();
                        match parser_guard.feed(&chunk) {
                            Ok(jsons) => {
                                drop(parser_guard); // Release parser lock before processing events
                                for json in jsons {
                                    let mut tool_acc_guard = tool_acc.lock().unwrap();
                                    if let Some(event) = parse_kiro_event_with_accumulator(
                                        &json,
                                        &mut tool_acc_guard,
                                    ) {
                                        drop(tool_acc_guard); // Release lock
                                                              // Process content events through thinking parser
                                        if event.event_type == "content" {
                                            if let Some(content) = &event.content {
                                                if let Some(ref tp_arc) = tp {
                                                    let mut tp_guard = tp_arc.lock().unwrap();
                                                    let parse_result = tp_guard.feed(content);

                                                    // Yield thinking content if any
                                                    if let Some(thinking) =
                                                        parse_result.thinking_content
                                                    {
                                                        let processed = tp_guard
                                                            .process_for_output(
                                                                &thinking,
                                                                parse_result
                                                                    .is_first_thinking_chunk,
                                                                parse_result.is_last_thinking_chunk,
                                                            );
                                                        if let Some(processed_thinking) = processed
                                                        {
                                                            events.push(Ok(KiroEvent {
                                                                event_type: "thinking".to_string(),
                                                                content: None,
                                                                thinking_content: Some(
                                                                    processed_thinking,
                                                                ),
                                                                tool_use: None,
                                                                usage: None,
                                                                context_usage_percentage: None,
                                                                is_first_thinking_chunk:
                                                                    parse_result
                                                                        .is_first_thinking_chunk,
                                                                is_last_thinking_chunk:
                                                                    parse_result
                                                                        .is_last_thinking_chunk,
                                                            }));
                                                        }
                                                    }

                                                    // Yield regular content if any
                                                    if let Some(regular) =
                                                        parse_result.regular_content
                                                    {
                                                        events.push(Ok(KiroEvent {
                                                            event_type: "content".to_string(),
                                                            content: Some(regular),
                                                            thinking_content: None,
                                                            tool_use: None,
                                                            usage: None,
                                                            context_usage_percentage: None,
                                                            is_first_thinking_chunk: false,
                                                            is_last_thinking_chunk: false,
                                                        }));
                                                    }
                                                } else {
                                                    // No thinking parser - pass through as-is
                                                    events.push(Ok(event));
                                                }
                                            }
                                        } else {
                                            // Non-content events pass through
                                            events.push(Ok(event));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                // Finalize pending tool before emitting parser error
                                let mut tool_acc_guard = tool_acc.lock().unwrap();
                                if let Some(completed_tool) = tool_acc_guard.finalize() {
                                    tracing::warn!(
                                        "Parser error - finalizing pending tool '{}'",
                                        completed_tool.name
                                    );
                                    events.push(Ok(KiroEvent {
                                        event_type: "tool_use".to_string(),
                                        content: None,
                                        thinking_content: None,
                                        tool_use: Some(completed_tool),
                                        usage: None,
                                        context_usage_percentage: None,
                                        is_first_thinking_chunk: false,
                                        is_last_thinking_chunk: false,
                                    }));
                                }
                                drop(tool_acc_guard);
                                events.push(Err(e));
                            }
                        }
                        futures::stream::iter(events)
                    }
                    Err(e) => {
                        // Finalize pending tool before emitting stream error
                        let mut events = Vec::new();
                        let mut tool_acc_guard = tool_acc.lock().unwrap();
                        if let Some(completed_tool) = tool_acc_guard.finalize() {
                            tracing::warn!(
                                "Stream error - finalizing pending tool '{}'",
                                completed_tool.name
                            );
                            events.push(Ok(KiroEvent {
                                event_type: "tool_use".to_string(),
                                content: None,
                                thinking_content: None,
                                tool_use: Some(completed_tool),
                                usage: None,
                                context_usage_percentage: None,
                                is_first_thinking_chunk: false,
                                is_last_thinking_chunk: false,
                            }));
                        }
                        drop(tool_acc_guard);
                        events.push(Err(ApiError::Internal(anyhow::anyhow!(
                            "Stream error: {}",
                            e
                        ))));
                        futures::stream::iter(events)
                    }
                }
            }
        })
        .flatten();

    // Clone tool accumulator for finalization stream
    let tool_accumulator_for_finalize = tool_accumulator.clone();

    // Create finalization stream that emits any remaining tool at end of stream
    let finalize_stream = futures::stream::unfold(
        Some(tool_accumulator_for_finalize),
        |state_opt| async move {
            let tool_acc_arc = state_opt?;
            let mut tool_acc = tool_acc_arc.lock().unwrap();

            // Finalize any remaining tool that didn't receive a stop event
            if let Some(completed_tool) = tool_acc.finalize() {
                tracing::debug!(
                    "Finalized remaining tool at stream end: {} with input keys: {:?}",
                    completed_tool.name,
                    completed_tool
                        .input
                        .as_object()
                        .map(|o| o.keys().collect::<Vec<_>>())
                );
                let event = KiroEvent {
                    event_type: "tool_use".to_string(),
                    content: None,
                    thinking_content: None,
                    tool_use: Some(completed_tool),
                    usage: None,
                    context_usage_percentage: None,
                    is_first_thinking_chunk: false,
                    is_last_thinking_chunk: false,
                };
                Some((Ok(event), None))
            } else {
                None
            }
        },
    );

    // Combine first chunk events with remaining stream, then finalization
    let combined = futures::stream::iter(events)
        .chain(remaining_stream)
        .chain(finalize_stream);

    Ok(combined.boxed())
}

impl Clone for SseParser {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            tool_accumulator: self.tool_accumulator.clone(),
        }
    }
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_parser_basic() {
        let mut parser = SseParser::new();

        // Use a pattern that matches EVENT_PATTERNS
        let chunk = b"{\"content\": \"Hello, world!\"}\n\n";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["content"], "Hello, world!");
    }

    #[test]
    fn test_sse_parser_done_marker() {
        let mut parser = SseParser::new();

        let chunk = b"data: [DONE]\n\n";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_sse_parser_aws_format() {
        let mut parser = SseParser::new();

        // Use a pattern that matches EVENT_PATTERNS
        let chunk =
            b":event-type: content\n:content-type: application/json\n{\"content\": \"Hello\"}\n\n";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["content"], "Hello");
    }

    #[test]
    fn test_parse_kiro_event_content() {
        let json = serde_json::json!({
            "contentBlockDelta": {
                "delta": {
                    "text": "Hello, world!"
                }
            }
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "content");
        assert_eq!(event.content, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_parse_kiro_event_tool_use() {
        let json = serde_json::json!({
            "contentBlockDelta": {
                "delta": {
                    "toolUse": {
                        "toolUseId": "call_123",
                        "name": "get_weather",
                        "input": {"location": "SF"}
                    }
                }
            }
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "tool_use");
        assert!(event.tool_use.is_some());
        let tool_use = event.tool_use.unwrap();
        assert_eq!(tool_use.tool_use_id, "call_123");
        assert_eq!(tool_use.name, "get_weather");
    }

    #[test]
    fn test_parse_kiro_event_usage() {
        let json = serde_json::json!({
            "metadata": {
                "usage": {
                    "inputTokens": 100,
                    "outputTokens": 50
                }
            }
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "usage");
        assert!(event.usage.is_some());
        let usage = event.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    // ==================== Tool Call Accumulator Tests ====================

    #[test]
    fn test_tool_call_accumulator_simple() {
        let mut acc = ToolCallAccumulator::new();

        // Tool start
        let start = serde_json::json!({
            "name": "get_weather",
            "toolUseId": "call_123"
        });
        let result = acc.process_event(&start);
        assert!(result.is_none()); // Not complete yet

        // Tool input
        let input = serde_json::json!({
            "input": "{\"location\": \"SF\"}"
        });
        let result = acc.process_event(&input);
        assert!(result.is_none()); // Not complete yet

        // Tool stop
        let stop = serde_json::json!({
            "stop": true
        });
        let result = acc.process_event(&stop);
        assert!(result.is_some());

        let tool = result.unwrap();
        assert_eq!(tool.name, "get_weather");
        assert_eq!(tool.tool_use_id, "call_123");
    }

    #[test]
    fn test_tool_call_accumulator_with_input_in_start() {
        let mut acc = ToolCallAccumulator::new();

        // Tool start with input
        let start = serde_json::json!({
            "name": "bash",
            "toolUseId": "call_456",
            "input": "{\"command\": \"ls\"}"
        });
        let result = acc.process_event(&start);
        assert!(result.is_none());

        // Tool stop
        let stop = serde_json::json!({
            "stop": true
        });
        let result = acc.process_event(&stop);
        assert!(result.is_some());

        let tool = result.unwrap();
        assert_eq!(tool.name, "bash");
        assert_eq!(tool.input["command"], "ls");
    }

    #[test]
    fn test_tool_call_accumulator_continuation_same_id() {
        let mut acc = ToolCallAccumulator::new();

        // First chunk with name and toolUseId
        let chunk1 = serde_json::json!({
            "name": "bash",
            "toolUseId": "call_789",
            "input": "{\"comm"
        });
        acc.process_event(&chunk1);

        // Continuation with same toolUseId (Kiro sends name in every chunk)
        let chunk2 = serde_json::json!({
            "name": "bash",
            "toolUseId": "call_789",
            "input": "and\": \"ls -la\"}"
        });
        acc.process_event(&chunk2);

        // Stop
        let stop = serde_json::json!({
            "stop": true
        });
        let result = acc.process_event(&stop);
        assert!(result.is_some());

        let tool = result.unwrap();
        assert_eq!(tool.name, "bash");
        assert_eq!(tool.input["command"], "ls -la");
    }

    #[test]
    fn test_tool_call_accumulator_finalize() {
        let mut acc = ToolCallAccumulator::new();

        // Start a tool but don't stop it
        let start = serde_json::json!({
            "name": "test_tool",
            "toolUseId": "call_999",
            "input": "{\"key\": \"value\"}"
        });
        acc.process_event(&start);

        // Finalize should complete the tool
        let result = acc.finalize();
        assert!(result.is_some());

        let tool = result.unwrap();
        assert_eq!(tool.name, "test_tool");
    }

    // ==================== Deduplicate Tool Calls Tests ====================

    #[test]
    fn test_deduplicate_tool_calls_empty() {
        let result = deduplicate_tool_calls(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_deduplicate_tool_calls_by_id() {
        let tools = vec![
            ToolUse {
                tool_use_id: "call_1".to_string(),
                name: "test".to_string(),
                input: serde_json::json!({}),
                truncation_info: None,
            },
            ToolUse {
                tool_use_id: "call_1".to_string(),
                name: "test".to_string(),
                input: serde_json::json!({"key": "value"}),
                truncation_info: None,
            },
        ];

        let result = deduplicate_tool_calls(tools);
        assert_eq!(result.len(), 1);
        // Should keep the one with more arguments
        assert_eq!(result[0].input["key"], "value");
    }

    #[test]
    fn test_deduplicate_tool_calls_by_name_args() {
        let tools = vec![
            ToolUse {
                tool_use_id: "call_1".to_string(),
                name: "test".to_string(),
                input: serde_json::json!({"key": "value"}),
                truncation_info: None,
            },
            ToolUse {
                tool_use_id: "call_2".to_string(),
                name: "test".to_string(),
                input: serde_json::json!({"key": "value"}),
                truncation_info: None,
            },
        ];

        let result = deduplicate_tool_calls(tools);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_deduplicate_tool_calls_different_tools() {
        let tools = vec![
            ToolUse {
                tool_use_id: "call_1".to_string(),
                name: "tool_a".to_string(),
                input: serde_json::json!({"key": "value1"}),
                truncation_info: None,
            },
            ToolUse {
                tool_use_id: "call_2".to_string(),
                name: "tool_b".to_string(),
                input: serde_json::json!({"key": "value2"}),
                truncation_info: None,
            },
        ];

        let result = deduplicate_tool_calls(tools);
        assert_eq!(result.len(), 2);
    }

    // ==================== SSE Parser Additional Tests ====================

    #[test]
    fn test_sse_parser_multiple_events() {
        let mut parser = SseParser::new();

        let chunk = b"{\"content\": \"Hello\"}{\"content\": \"World\"}";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["content"], "Hello");
        assert_eq!(events[1]["content"], "World");
    }

    #[test]
    fn test_sse_parser_partial_json() {
        let mut parser = SseParser::new();

        // First chunk - incomplete JSON
        let chunk1 = b"{\"content\": \"Hel";
        let events1 = parser.feed(chunk1).unwrap();
        assert_eq!(events1.len(), 0);

        // Second chunk - completes the JSON
        let chunk2 = b"lo\"}";
        let events2 = parser.feed(chunk2).unwrap();
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0]["content"], "Hello");
    }

    #[test]
    fn test_sse_parser_nested_json() {
        let mut parser = SseParser::new();

        let chunk = b"{\"content\": \"{\\\"nested\\\": true}\"}";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["content"], "{\"nested\": true}");
    }

    #[test]
    fn test_sse_parser_usage_event() {
        let mut parser = SseParser::new();

        let chunk = b"{\"usage\": {\"inputTokens\": 100, \"outputTokens\": 50}}";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["usage"]["inputTokens"], 100);
    }

    #[test]
    fn test_sse_parser_context_usage() {
        let mut parser = SseParser::new();

        let chunk = b"{\"contextUsagePercentage\": 45.5}";
        let events = parser.feed(chunk).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["contextUsagePercentage"], 45.5);
    }

    // ==================== Parse Kiro Event Additional Tests ====================

    #[test]
    fn test_parse_kiro_event_direct_content() {
        let json = serde_json::json!({
            "content": "Direct content"
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "content");
        assert_eq!(event.content, Some("Direct content".to_string()));
    }

    #[test]
    fn test_parse_kiro_event_direct_usage_number() {
        let json = serde_json::json!({
            "usage": 1.5
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "usage");
        assert!(event.usage.is_some());
    }

    #[test]
    fn test_parse_kiro_event_direct_usage_object() {
        let json = serde_json::json!({
            "usage": {
                "inputTokens": 200,
                "outputTokens": 100
            }
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "usage");
        let usage = event.usage.unwrap();
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
    }

    #[test]
    fn test_parse_kiro_event_context_usage() {
        let json = serde_json::json!({
            "contextUsagePercentage": 75.0
        });

        let event = parse_kiro_event(&json).unwrap();
        assert_eq!(event.event_type, "context_usage");
        assert_eq!(event.context_usage_percentage, Some(75.0));
    }

    #[test]
    fn test_parse_kiro_event_followup_prompt_ignored() {
        let json = serde_json::json!({
            "followupPrompt": "Some prompt"
        });

        let event = parse_kiro_event(&json);
        assert!(event.is_none());
    }

    #[test]
    fn test_parse_kiro_event_message_stop_ignored() {
        let json = serde_json::json!({
            "messageStop": {}
        });

        let event = parse_kiro_event(&json);
        assert!(event.is_none());
    }

    // ==================== KiroEvent Tests ====================

    #[test]
    fn test_kiro_event_default() {
        let event = KiroEvent {
            event_type: "content".to_string(),
            content: Some("test".to_string()),
            thinking_content: None,
            tool_use: None,
            usage: None,
            context_usage_percentage: None,
            is_first_thinking_chunk: false,
            is_last_thinking_chunk: false,
        };

        assert_eq!(event.event_type, "content");
        assert!(!event.is_first_thinking_chunk);
        assert!(!event.is_last_thinking_chunk);
    }

    #[test]
    fn test_tool_use_serialization() {
        let tool_use = ToolUse {
            tool_use_id: "call_123".to_string(),
            name: "test_tool".to_string(),
            input: serde_json::json!({"key": "value"}),
            truncation_info: None,
        };

        let json = serde_json::to_string(&tool_use).unwrap();
        assert!(json.contains("call_123"));
        assert!(json.contains("test_tool"));
    }

    #[test]
    fn test_usage_serialization() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
        };

        let json = serde_json::to_string(&usage).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.input_tokens, 100);
        assert_eq!(parsed.output_tokens, 50);
    }

    // ==================== Finalize Double-Call Prevention Tests ====================

    #[test]
    fn test_tool_call_accumulator_finalize_prevents_double() {
        let mut acc = ToolCallAccumulator::new();

        // Start a tool but don't stop it
        let start = serde_json::json!({
            "name": "write",
            "toolUseId": "call_double_test",
            "input": "{\"filePath\": \"/test/path.txt\"}"
        });
        acc.process_event(&start);

        // First finalize should return the tool
        let result1 = acc.finalize();
        assert!(result1.is_some());
        let tool = result1.unwrap();
        assert_eq!(tool.name, "write");
        assert_eq!(tool.tool_use_id, "call_double_test");

        // Second finalize should return None (prevents double-finalization)
        let result2 = acc.finalize();
        assert!(result2.is_none());

        // Third finalize should also return None
        let result3 = acc.finalize();
        assert!(result3.is_none());
    }

    #[test]
    fn test_tool_call_accumulator_truncated_json() {
        let mut acc = ToolCallAccumulator::new();

        // Simulate truncated JSON input (stream terminated mid-tool-call)
        let start = serde_json::json!({
            "name": "write",
            "toolUseId": "call_truncated",
            "input": "{\"filePath\": \"/Users/test/docs/api.yaml\""
        });
        // Note: The input JSON is missing the closing "}"
        acc.process_event(&start);

        // Finalize should still return the tool with whatever data we have
        let result = acc.finalize();
        assert!(result.is_some());

        let tool = result.unwrap();
        assert_eq!(tool.name, "write");
        assert_eq!(tool.tool_use_id, "call_truncated");
        // The input should be an empty object since the truncated JSON fails to parse
        assert!(tool.input.is_object());
    }
}

// ==================================================================================================
// OpenAI Streaming
// ==================================================================================================

use crate::models::openai::{
    ChatCompletionChunk, ChatCompletionChunkChoice, ChatCompletionChunkDelta, ChatCompletionUsage,
    FunctionCallDelta, ToolCallDelta,
};
use futures::stream::BoxStream;
use uuid::Uuid;

/// Generates a unique completion ID in OpenAI format.
fn generate_completion_id() -> String {
    format!("chatcmpl-{}", &Uuid::new_v4().simple().to_string()[..24])
}

/// Converts Kiro stream to OpenAI SSE format.
///
/// This function takes a Kiro API response stream and converts it to OpenAI's
/// chat.completion.chunk format with SSE encoding.
pub async fn stream_kiro_to_openai(
    response: reqwest::Response,
    model: &str,
    first_token_timeout_secs: u64,
    input_tokens: i32,
    output_tokens_tracker: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    include_usage: bool,
    truncation_recovery: bool,
) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError> {
    let completion_id = generate_completion_id();
    let created_time = chrono::Utc::now().timestamp();
    let model = model.to_string();

    // Parse Kiro stream and collect all events
    let kiro_stream = parse_kiro_stream(response, first_token_timeout_secs).await?;

    // Use scan to maintain state across stream items
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StreamState {
        first_chunk: bool,
        tool_calls: Vec<ToolUse>,
        usage: Option<Usage>,
        accumulated_text: String, // Accumulate all text for accurate token counting
    }

    let state = Arc::new(Mutex::new(StreamState {
        first_chunk: true,
        tool_calls: Vec::new(),
        usage: None,
        accumulated_text: String::new(),
    }));

    let completion_id_clone = completion_id.clone();
    let model_clone = model.clone();

    // Clone state for use in final stream
    let state_for_final = state.clone();

    // Clone tracker for stream processing
    let tracker_for_stream = output_tokens_tracker.clone();

    // Convert to OpenAI chunks
    let openai_stream = kiro_stream.filter_map(move |event_result| {
        let completion_id = completion_id_clone.clone();
        let model = model_clone.clone();
        let state = state.clone();
        let tracker = tracker_for_stream.clone();

        async move {
            match event_result {
                Ok(event) => {
                    let mut state = state.lock().unwrap();

                    match event.event_type.as_str() {
                        "content" => {
                            if let Some(content) = event.content {
                                // Accumulate text for accurate token counting
                                state.accumulated_text.push_str(&content);

                                let delta = ChatCompletionChunkDelta {
                                    role: if state.first_chunk {
                                        Some("assistant".to_string())
                                    } else {
                                        None
                                    },
                                    content: Some(content),
                                    tool_calls: None,
                                    reasoning_content: None,
                                };

                                state.first_chunk = false;

                                let chunk = ChatCompletionChunk {
                                    id: completion_id,
                                    object: "chat.completion.chunk".to_string(),
                                    created: created_time,
                                    model,
                                    choices: vec![ChatCompletionChunkChoice {
                                        index: 0,
                                        delta,
                                        finish_reason: None,
                                        logprobs: None,
                                    }],
                                    usage: None,
                                    system_fingerprint: None,
                                };

                                let json = serde_json::to_string(&chunk)
                                    .unwrap_or_else(|_| "{}".to_string());
                                Some(Ok(format!("data: {}\n\n", json)))
                            } else {
                                None
                            }
                        }
                        "thinking" => {
                            if let Some(thinking) = event.thinking_content {
                                // Accumulate text for accurate token counting
                                state.accumulated_text.push_str(&thinking);

                                let delta = ChatCompletionChunkDelta {
                                    role: if state.first_chunk {
                                        Some("assistant".to_string())
                                    } else {
                                        None
                                    },
                                    content: None,
                                    tool_calls: None,
                                    reasoning_content: Some(thinking),
                                };

                                state.first_chunk = false;

                                let chunk = ChatCompletionChunk {
                                    id: completion_id,
                                    object: "chat.completion.chunk".to_string(),
                                    created: created_time,
                                    model,
                                    choices: vec![ChatCompletionChunkChoice {
                                        index: 0,
                                        delta,
                                        finish_reason: None,
                                        logprobs: None,
                                    }],
                                    usage: None,
                                    system_fingerprint: None,
                                };

                                let json = serde_json::to_string(&chunk)
                                    .unwrap_or_else(|_| "{}".to_string());
                                Some(Ok(format!("data: {}\n\n", json)))
                            } else {
                                None
                            }
                        }
                        "tool_use" => {
                            if let Some(tool_use) = event.tool_use {
                                state.tool_calls.push(tool_use);
                            }
                            None
                        }
                        "usage" => {
                            if let Some(u) = event.usage {
                                state.usage = Some(u.clone());
                                if let Some(ref t) = tracker {
                                    t.store(
                                        u.output_tokens as u64,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                }
                            }
                            None
                        }
                        _ => None,
                    }
                }
                Err(e) => Some(Err(e)),
            }
        }
    });

    // Add final chunk with tool calls, usage, and [DONE]
    let completion_id_for_final = completion_id.clone();
    let model_for_final = model.clone();
    let tracker_for_final = output_tokens_tracker.clone();
    let final_chunks_stream = futures::stream::unfold(
        Some((
            state_for_final,
            completion_id_for_final,
            model_for_final,
            created_time,
            input_tokens,
            tracker_for_final,
            truncation_recovery,
        )),
        move |state_opt| async move {
            let (state_arc, completion_id, model, created_time, input_tokens, tracker, truncation_recovery) = state_opt?;
            let state = state_arc.lock().unwrap();
            let mut final_chunks = Vec::new();

            // Deduplicate tool calls before sending
            let deduped_tool_calls = deduplicate_tool_calls(state.tool_calls.clone());

            // Save truncation state for recovery on next request
            if truncation_recovery {
                for tc in &deduped_tool_calls {
                    if let Some(ref info) = tc.truncation_info {
                        crate::truncation::TRUNCATION_STATE.save_tool_truncation(
                            &tc.tool_use_id,
                            &tc.name,
                            info.clone(),
                        );
                    }
                }
                // Detect content truncation: no usage + has content + no tools
                if state.usage.is_none()
                    && !state.accumulated_text.is_empty()
                    && deduped_tool_calls.is_empty()
                {
                    crate::truncation::TRUNCATION_STATE
                        .save_content_truncation(&state.accumulated_text);
                }
            }

            // Send tool calls if present
            if !deduped_tool_calls.is_empty() {
                let tool_call_deltas: Vec<ToolCallDelta> = deduped_tool_calls
                    .iter()
                    .enumerate()
                    .map(|(idx, tc)| ToolCallDelta {
                        index: idx as i32,
                        id: Some(tc.tool_use_id.clone()),
                        tool_type: Some("function".to_string()),
                        function: Some(FunctionCallDelta {
                            name: Some(tc.name.clone()),
                            arguments: Some(
                                serde_json::to_string(&tc.input)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            ),
                        }),
                    })
                    .collect();

                let tool_chunk = ChatCompletionChunk {
                    id: completion_id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created: created_time,
                    model: model.clone(),
                    choices: vec![ChatCompletionChunkChoice {
                        index: 0,
                        delta: ChatCompletionChunkDelta {
                            role: None,
                            content: None,
                            tool_calls: Some(tool_call_deltas),
                            reasoning_content: None,
                        },
                        finish_reason: None,
                        logprobs: None,
                    }],
                    usage: None,
                    system_fingerprint: None,
                };

                let json = serde_json::to_string(&tool_chunk).unwrap_or_else(|_| "{}".to_string());
                final_chunks.push(Ok(format!("data: {}\n\n", json)));
            }

            // Determine finish_reason
            let finish_reason = if !deduped_tool_calls.is_empty() {
                "tool_calls"
            } else {
                "stop"
            };

            // Calculate usage - use our calculated input_tokens, output from Kiro
            // Only include usage if explicitly requested via stream_options.include_usage
            let usage_obj = if include_usage {
                if let Some(ref u) = state.usage {
                    tracing::info!(
                        "Including usage in final chunk: prompt_tokens={}, completion_tokens={}, total_tokens={}",
                        input_tokens,
                        u.output_tokens,
                        input_tokens + u.output_tokens
                    );
                    Some(ChatCompletionUsage {
                        prompt_tokens: input_tokens,
                        completion_tokens: u.output_tokens,
                        total_tokens: input_tokens + u.output_tokens,
                        credits_used: None,
                        prompt_tokens_details: None,
                    })
                } else {
                    // Fallback: Count output tokens using tiktoken (same method as input tokens)
                    let output_tokens = crate::tokenizer::count_tokens(&state.accumulated_text, false);

                    if output_tokens > 0 {
                        tracing::info!(
                            "No usage data from Kiro API - using tiktoken count: prompt_tokens={}, completion_tokens={} (counted from {} chars), total_tokens={}",
                            input_tokens,
                            output_tokens,
                            state.accumulated_text.len(),
                            input_tokens + output_tokens
                        );

                        // Update metrics tracker with counted tokens
                        if let Some(ref t) = tracker {
                            t.store(output_tokens as u64, std::sync::atomic::Ordering::Relaxed);
                        }

                        Some(ChatCompletionUsage {
                            prompt_tokens: input_tokens,
                            completion_tokens: output_tokens,
                            total_tokens: input_tokens + output_tokens,
                            credits_used: None,
                            prompt_tokens_details: None,
                        })
                    } else {
                        tracing::warn!("include_usage=true but no usage data received from Kiro API and no content to count from");
                        None
                    }
                }
            } else {
                tracing::debug!("Excluding usage from final chunk (include_usage=false)");
                None
            };

            // Final chunk with finish_reason and usage
            let final_chunk = ChatCompletionChunk {
                id: completion_id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: created_time,
                model: model.clone(),
                choices: vec![ChatCompletionChunkChoice {
                    index: 0,
                    delta: ChatCompletionChunkDelta {
                        role: None,
                        content: None,
                        tool_calls: None,
                        reasoning_content: None,
                    },
                    finish_reason: Some(finish_reason.to_string()),
                    logprobs: None,
                }],
                usage: usage_obj,
                system_fingerprint: None,
            };

            let json = serde_json::to_string(&final_chunk).unwrap_or_else(|_| "{}".to_string());
            final_chunks.push(Ok(format!("data: {}\n\n", json)));

            // [DONE] marker
            final_chunks.push(Ok("data: [DONE]\n\n".to_string()));

            Some((futures::stream::iter(final_chunks), None))
        },
    )
    .flatten();

    let final_stream = openai_stream.chain(final_chunks_stream);

    Ok(final_stream.boxed())
}

// ==================================================================================================
// Anthropic Streaming
// ==================================================================================================

/// Formats data as Anthropic SSE event.
///
/// Anthropic SSE format:
/// ```text
/// event: {event_type}
/// data: {json_data}
///
/// ```
fn format_anthropic_sse_event(event_type: &str, data: &Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        event_type,
        serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string())
    )
}

/// Generates a unique message ID in Anthropic format.
fn generate_anthropic_message_id() -> String {
    format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24])
}

/// Converts Kiro stream to Anthropic SSE format.
///
/// This function takes a Kiro API response stream and converts it to Anthropic's
/// Messages API streaming format with SSE encoding.
pub async fn stream_kiro_to_anthropic(
    response: reqwest::Response,
    model: &str,
    first_token_timeout_secs: u64,
    input_tokens: i32,
    output_tokens_tracker: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    truncation_recovery: bool,
) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError> {
    let message_id = generate_anthropic_message_id();
    let model = model.to_string();

    // Parse Kiro stream
    let kiro_stream = parse_kiro_stream(response, first_token_timeout_secs).await?;

    // Use state to track blocks
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StreamState {
        text_block_started: bool,
        text_block_index: i32,
        thinking_block_started: bool,
        thinking_block_index: i32,
        current_block_index: i32,
        tool_calls: Vec<ToolUse>,
        usage: Option<Usage>,
        accumulated_text: String, // Accumulate all text for accurate token counting
    }

    let state = Arc::new(Mutex::new(StreamState::default()));

    // Send message_start event first
    let message_start = serde_json::json!({
        "type": "message_start",
        "message": {
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": model,
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": 0
            }
        }
    });

    let start_event = format_anthropic_sse_event("message_start", &message_start);

    let _model_clone = model.clone();

    // Clone state for use in final stream
    let state_for_final = state.clone();

    // Clone tracker for final events before it gets moved
    let tracker_for_final = output_tokens_tracker.clone();

    // Convert Kiro events to Anthropic events
    let anthropic_stream = kiro_stream.filter_map(move |event_result| {
        let _model = _model_clone.clone();
        let state = state.clone();
        let tracker = output_tokens_tracker.clone();

        async move {
            match event_result {
                Ok(event) => {
                    let mut state = state.lock().unwrap();

                    match event.event_type.as_str() {
                        "content" => {
                            if let Some(content) = event.content {
                                // Accumulate text for accurate token counting
                                state.accumulated_text.push_str(&content);

                                // Start text block if not started
                                if !state.text_block_started {
                                    state.text_block_index = state.current_block_index;
                                    state.current_block_index += 1;
                                    state.text_block_started = true;

                                    let block_start = serde_json::json!({
                                        "type": "content_block_start",
                                        "index": state.text_block_index,
                                        "content_block": {
                                            "type": "text",
                                            "text": ""
                                        }
                                    });

                                    let start_event = format_anthropic_sse_event(
                                        "content_block_start",
                                        &block_start,
                                    );

                                    // Send both start and delta
                                    let delta = serde_json::json!({
                                        "type": "content_block_delta",
                                        "index": state.text_block_index,
                                        "delta": {
                                            "type": "text_delta",
                                            "text": content
                                        }
                                    });

                                    let delta_event =
                                        format_anthropic_sse_event("content_block_delta", &delta);

                                    return Some(Ok(format!("{}{}", start_event, delta_event)));
                                } else {
                                    // Send delta only
                                    let delta = serde_json::json!({
                                        "type": "content_block_delta",
                                        "index": state.text_block_index,
                                        "delta": {
                                            "type": "text_delta",
                                            "text": content
                                        }
                                    });

                                    return Some(Ok(format_anthropic_sse_event(
                                        "content_block_delta",
                                        &delta,
                                    )));
                                }
                            }
                            None
                        }
                        "thinking" => {
                            if let Some(thinking) = event.thinking_content {
                                // Accumulate text for accurate token counting
                                state.accumulated_text.push_str(&thinking);

                                // Start thinking block if not started
                                if !state.thinking_block_started {
                                    state.thinking_block_index = state.current_block_index;
                                    state.current_block_index += 1;
                                    state.thinking_block_started = true;

                                    let block_start = serde_json::json!({
                                        "type": "content_block_start",
                                        "index": state.thinking_block_index,
                                        "content_block": {
                                            "type": "thinking",
                                            "thinking": ""
                                        }
                                    });

                                    let start_event = format_anthropic_sse_event(
                                        "content_block_start",
                                        &block_start,
                                    );

                                    // Send both start and delta
                                    let delta = serde_json::json!({
                                        "type": "content_block_delta",
                                        "index": state.thinking_block_index,
                                        "delta": {
                                            "type": "thinking_delta",
                                            "thinking": thinking
                                        }
                                    });

                                    let delta_event =
                                        format_anthropic_sse_event("content_block_delta", &delta);

                                    return Some(Ok(format!("{}{}", start_event, delta_event)));
                                } else {
                                    // Send delta only
                                    let delta = serde_json::json!({
                                        "type": "content_block_delta",
                                        "index": state.thinking_block_index,
                                        "delta": {
                                            "type": "thinking_delta",
                                            "thinking": thinking
                                        }
                                    });

                                    return Some(Ok(format_anthropic_sse_event(
                                        "content_block_delta",
                                        &delta,
                                    )));
                                }
                            }
                            None
                        }
                        "tool_use" => {
                            if let Some(tool_use) = event.tool_use {
                                tracing::info!(
                                    "Received tool_use event: name={}, id={}, total_collected={}",
                                    tool_use.name,
                                    tool_use.tool_use_id,
                                    state.tool_calls.len() + 1
                                );
                                state.tool_calls.push(tool_use);
                            }
                            None
                        }
                        "usage" => {
                            if let Some(u) = event.usage {
                                state.usage = Some(u.clone());
                                if let Some(ref t) = tracker {
                                    t.store(
                                        u.output_tokens as u64,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                }
                            }
                            None
                        }
                        _ => None,
                    }
                }
                Err(e) => Some(Err(e)),
            }
        }
    });

    // Add final events
    let final_events_stream = futures::stream::unfold(Some((state_for_final, truncation_recovery)), move |state_opt| {
        let tracker = tracker_for_final.clone();
        async move {
            let (state_arc, truncation_recovery) = state_opt?;
            let state = state_arc.lock().unwrap();
        let mut final_events = Vec::new();

        // Close thinking block if open
        if state.thinking_block_started {
            let block_stop = serde_json::json!({
                "type": "content_block_stop",
                "index": state.thinking_block_index
            });
            final_events.push(Ok(format_anthropic_sse_event("content_block_stop", &block_stop)));
        }

        // Close text block if open
        if state.text_block_started {
            let block_stop = serde_json::json!({
                "type": "content_block_stop",
                "index": state.text_block_index
            });
            final_events.push(Ok(format_anthropic_sse_event("content_block_stop", &block_stop)));
        }

        // Deduplicate tool calls before sending
        let deduped_tool_calls = deduplicate_tool_calls(state.tool_calls.clone());

        // Save truncation state for recovery on next request
        if truncation_recovery {
            for tc in &deduped_tool_calls {
                if let Some(ref info) = tc.truncation_info {
                    crate::truncation::TRUNCATION_STATE.save_tool_truncation(
                        &tc.tool_use_id,
                        &tc.name,
                        info.clone(),
                    );
                }
            }
            // Detect content truncation: no usage + has content + no tools
            if state.usage.is_none()
                && !state.accumulated_text.is_empty()
                && deduped_tool_calls.is_empty()
            {
                crate::truncation::TRUNCATION_STATE
                    .save_content_truncation(&state.accumulated_text);
            }
        }

        // Log tool calls for debugging
        if !deduped_tool_calls.is_empty() {
            tracing::info!(
                "Emitting {} tool calls (before dedup: {}): {:?}",
                deduped_tool_calls.len(),
                state.tool_calls.len(),
                deduped_tool_calls.iter().map(|t| format!("{}:{}", t.name, &t.tool_use_id)).collect::<Vec<_>>()
            );
        }

        // Send tool use blocks if present
        let mut tool_block_index = state.current_block_index;
        for tool_use in &deduped_tool_calls {
            let tool_index = tool_block_index;
            tool_block_index += 1;  // Increment for each tool

            let block_start = serde_json::json!({
                "type": "content_block_start",
                "index": tool_index,
                "content_block": {
                    "type": "tool_use",
                    "id": tool_use.tool_use_id,
                    "name": tool_use.name,
                    "input": {}
                }
            });
            final_events.push(Ok(format_anthropic_sse_event("content_block_start", &block_start)));

            let delta = serde_json::json!({
                "type": "content_block_delta",
                "index": tool_index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": serde_json::to_string(&tool_use.input).unwrap_or_else(|_| "{}".to_string())
                }
            });
            final_events.push(Ok(format_anthropic_sse_event("content_block_delta", &delta)));

            let block_stop = serde_json::json!({
                "type": "content_block_stop",
                "index": tool_index
            });
            final_events.push(Ok(format_anthropic_sse_event("content_block_stop", &block_stop)));
        }

        // Determine stop_reason
        let stop_reason = if !deduped_tool_calls.is_empty() {
            "tool_use"
        } else {
            "end_turn"
        };

        // Calculate usage
        let output_tokens = if let Some(ref u) = state.usage {
            u.output_tokens
        } else {
            // Fallback: Count output tokens using tiktoken (same method as input tokens)
            let tokens = crate::tokenizer::count_tokens(&state.accumulated_text, false);
            if tokens > 0 {
                tracing::info!(
                    "No usage data from Kiro API - using tiktoken count: output_tokens={} (counted from {} chars)",
                    tokens,
                    state.accumulated_text.len()
                );

                // Update metrics tracker with counted tokens
                if let Some(ref t) = tracker {
                    t.store(tokens as u64, std::sync::atomic::Ordering::Relaxed);
                }
            }
            tokens
        };

        // Send message_delta with stop_reason
        let message_delta = serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            },
            "usage": {
                "output_tokens": output_tokens
            }
        });
        final_events.push(Ok(format_anthropic_sse_event("message_delta", &message_delta)));

        // Send message_stop
        let message_stop = serde_json::json!({
            "type": "message_stop"
        });
        final_events.push(Ok(format_anthropic_sse_event("message_stop", &message_stop)));

        Some((futures::stream::iter(final_events), None))
    }})
    .flatten();

    let final_stream = anthropic_stream.chain(final_events_stream);

    // Prepend message_start event
    let complete_stream = futures::stream::once(async move { Ok(start_event) }).chain(final_stream);

    Ok(complete_stream.boxed())
}

// ==================================================================================================
// Non-Streaming Response Collection (like Python's collect_stream_response)
// ==================================================================================================

/// Collects a complete OpenAI response from a Kiro stream.
///
/// This is used for non-streaming mode - it processes the stream internally
/// and returns a single complete response. This matches Python's collect_stream_response().
///
/// Kiro API always returns AWS Event Stream format, even for non-streaming requests.
/// This function parses that stream and aggregates all content into a single response.
pub async fn collect_openai_response(
    response: reqwest::Response,
    model: &str,
    first_token_timeout_secs: u64,
    input_tokens: i32,
    truncation_recovery: bool,
) -> Result<Value, ApiError> {
    use futures::StreamExt;

    let completion_id = generate_completion_id();
    let created_time = chrono::Utc::now().timestamp();

    // Parse Kiro stream
    let mut kiro_stream = parse_kiro_stream(response, first_token_timeout_secs).await?;

    // Collect all content
    let mut full_content = String::new();
    let mut full_reasoning_content = String::new();
    let mut tool_calls: Vec<ToolUse> = Vec::new();
    let mut usage: Option<Usage> = None;

    while let Some(event_result) = kiro_stream.next().await {
        match event_result {
            Ok(event) => match event.event_type.as_str() {
                "content" => {
                    if let Some(content) = event.content {
                        full_content.push_str(&content);
                    }
                }
                "thinking" => {
                    if let Some(thinking) = event.thinking_content {
                        full_reasoning_content.push_str(&thinking);
                    }
                }
                "tool_use" => {
                    if let Some(tool_use) = event.tool_use {
                        tool_calls.push(tool_use);
                    }
                }
                "usage" => {
                    if let Some(u) = event.usage {
                        usage = Some(u);
                    }
                }
                _ => {}
            },
            Err(e) => {
                tracing::warn!("Error in stream: {:?}", e);
            }
        }
    }

    // Deduplicate tool calls
    let tool_calls = deduplicate_tool_calls(tool_calls);

    // Save truncation state for recovery on next request
    if truncation_recovery {
        for tc in &tool_calls {
            if let Some(ref info) = tc.truncation_info {
                crate::truncation::TRUNCATION_STATE.save_tool_truncation(
                    &tc.tool_use_id,
                    &tc.name,
                    info.clone(),
                );
            }
        }
        if usage.is_none() && !full_content.is_empty() && tool_calls.is_empty() {
            crate::truncation::TRUNCATION_STATE.save_content_truncation(&full_content);
        }
    }

    // Build message
    let mut message = serde_json::json!({
        "role": "assistant",
        "content": full_content
    });

    if !full_reasoning_content.is_empty() {
        message["reasoning_content"] = serde_json::json!(full_reasoning_content);
    }

    if !tool_calls.is_empty() {
        let tool_calls_json: Vec<Value> = tool_calls
            .iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.tool_use_id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": serde_json::to_string(&tc.input).unwrap_or_else(|_| "{}".to_string())
                    }
                })
            })
            .collect();
        message["tool_calls"] = serde_json::json!(tool_calls_json);
    }

    // Determine finish_reason
    let finish_reason = if !tool_calls.is_empty() {
        "tool_calls"
    } else {
        "stop"
    };

    // Build usage - use our calculated input_tokens, output from Kiro
    let output_tokens = if let Some(u) = usage {
        u.output_tokens
    } else {
        // Fallback: Count output tokens using tiktoken (same method as input tokens)
        let mut accumulated_text = full_content.clone();
        accumulated_text.push_str(&full_reasoning_content);
        let tokens = crate::tokenizer::count_tokens(&accumulated_text, false);
        if tokens > 0 {
            tracing::info!(
                "No usage data from Kiro API - using tiktoken count: output_tokens={} (counted from {} chars)",
                tokens,
                accumulated_text.len()
            );
        }
        tokens
    };

    let usage_json = serde_json::json!({
        "prompt_tokens": input_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens
    });

    // Build complete response
    let response = serde_json::json!({
        "id": completion_id,
        "object": "chat.completion",
        "created": created_time,
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": usage_json
    });

    Ok(response)
}

/// Collects a complete Anthropic response from a Kiro stream.
///
/// This is used for non-streaming mode - it processes the stream internally
/// and returns a single complete response.
pub async fn collect_anthropic_response(
    response: reqwest::Response,
    model: &str,
    first_token_timeout_secs: u64,
    input_tokens: i32,
    truncation_recovery: bool,
) -> Result<Value, ApiError> {
    use futures::StreamExt;

    let message_id = generate_anthropic_message_id();

    // Parse Kiro stream
    let mut kiro_stream = parse_kiro_stream(response, first_token_timeout_secs).await?;

    // Collect all content
    let mut full_content = String::new();
    let mut full_thinking_content = String::new();
    let mut tool_calls: Vec<ToolUse> = Vec::new();
    let mut usage: Option<Usage> = None;

    while let Some(event_result) = kiro_stream.next().await {
        match event_result {
            Ok(event) => match event.event_type.as_str() {
                "content" => {
                    if let Some(content) = event.content {
                        full_content.push_str(&content);
                    }
                }
                "thinking" => {
                    if let Some(thinking) = event.thinking_content {
                        full_thinking_content.push_str(&thinking);
                    }
                }
                "tool_use" => {
                    if let Some(tool_use) = event.tool_use {
                        tool_calls.push(tool_use);
                    }
                }
                "usage" => {
                    if let Some(u) = event.usage {
                        usage = Some(u);
                    }
                }
                _ => {}
            },
            Err(e) => {
                tracing::warn!("Error in stream: {:?}", e);
            }
        }
    }

    // Deduplicate tool calls
    let tool_calls = deduplicate_tool_calls(tool_calls);

    // Save truncation state for recovery on next request
    if truncation_recovery {
        for tc in &tool_calls {
            if let Some(ref info) = tc.truncation_info {
                crate::truncation::TRUNCATION_STATE.save_tool_truncation(
                    &tc.tool_use_id,
                    &tc.name,
                    info.clone(),
                );
            }
        }
        if usage.is_none() && !full_content.is_empty() && tool_calls.is_empty() {
            crate::truncation::TRUNCATION_STATE.save_content_truncation(&full_content);
        }
    }

    // Build content blocks
    let mut content_blocks: Vec<Value> = Vec::new();

    // Add thinking block if present
    if !full_thinking_content.is_empty() {
        content_blocks.push(serde_json::json!({
            "type": "thinking",
            "thinking": full_thinking_content
        }));
    }

    // Add text block if present
    if !full_content.is_empty() {
        content_blocks.push(serde_json::json!({
            "type": "text",
            "text": full_content
        }));
    }

    // Add tool use blocks
    for tool_use in &tool_calls {
        content_blocks.push(serde_json::json!({
            "type": "tool_use",
            "id": tool_use.tool_use_id,
            "name": tool_use.name,
            "input": tool_use.input
        }));
    }

    // Determine stop_reason
    let stop_reason = if !tool_calls.is_empty() {
        "tool_use"
    } else {
        "end_turn"
    };

    // Build usage - use passed input_tokens, get output_tokens from stream
    let output_tokens = if let Some(u) = usage {
        u.output_tokens
    } else {
        // Fallback: Count output tokens using tiktoken (same method as input tokens)
        let mut accumulated_text = full_content.clone();
        accumulated_text.push_str(&full_thinking_content);
        let tokens = crate::tokenizer::count_tokens(&accumulated_text, false);
        if tokens > 0 {
            tracing::info!(
                "No usage data from Kiro API - using tiktoken count: output_tokens={} (counted from {} chars)",
                tokens,
                accumulated_text.len()
            );
        }
        tokens
    };

    // Build complete response
    let response = serde_json::json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    });

    Ok(response)
}
