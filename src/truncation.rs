//! Truncation recovery system for detecting and recovering from API response truncation.
//!
//! The Kiro API can silently truncate large responses mid-stream, especially tool call arguments.
//! This module provides:
//! - Truncation diagnosis (heuristic JSON truncation detection)
//! - Global state cache (stores truncation info between requests)
//! - Recovery message generation
//! - Injection functions (modify incoming messages on next request)

use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde_json::Value;
use sha2::{Digest, Sha256};

// ==================================================================================================
// Truncation Diagnosis
// ==================================================================================================

/// Information about whether a JSON string appears truncated.
#[derive(Debug, Clone)]
pub struct TruncationInfo {
    pub is_truncated: bool,
    pub reason: String,
    pub size_bytes: usize,
}

/// Diagnose whether a JSON string was truncated mid-stream.
///
/// Uses heuristic analysis matching Python's `_diagnose_json_truncation`:
/// - Empty string → not truncated
/// - Starts with `{` but doesn't end with `}` → truncated
/// - Starts with `[` but doesn't end with `]` → truncated
/// - Unbalanced braces → truncated
/// - Unbalanced brackets → truncated
/// - Unclosed string literal → truncated
pub fn diagnose_json_truncation(json_str: &str) -> TruncationInfo {
    let size_bytes = json_str.len();
    let trimmed = json_str.trim();

    // Empty string is not truncated
    if trimmed.is_empty() {
        return TruncationInfo {
            is_truncated: false,
            reason: "empty string".to_string(),
            size_bytes,
        };
    }

    // Check: starts with { but doesn't end with }
    if trimmed.starts_with('{') && !trimmed.ends_with('}') {
        return TruncationInfo {
            is_truncated: true,
            reason: "starts with '{' but does not end with '}'".to_string(),
            size_bytes,
        };
    }

    // Check: starts with [ but doesn't end with ]
    if trimmed.starts_with('[') && !trimmed.ends_with(']') {
        return TruncationInfo {
            is_truncated: true,
            reason: "starts with '[' but does not end with ']'".to_string(),
            size_bytes,
        };
    }

    // Count braces and brackets (simplified - doesn't handle braces inside strings perfectly)
    let mut brace_count: i32 = 0;
    let mut bracket_count: i32 = 0;

    for ch in trimmed.chars() {
        match ch {
            '{' => brace_count += 1,
            '}' => brace_count -= 1,
            '[' => bracket_count += 1,
            ']' => bracket_count -= 1,
            _ => {}
        }
    }

    if brace_count != 0 {
        return TruncationInfo {
            is_truncated: true,
            reason: format!("unbalanced braces (count: {})", brace_count),
            size_bytes,
        };
    }

    if bracket_count != 0 {
        return TruncationInfo {
            is_truncated: true,
            reason: format!("unbalanced brackets (count: {})", bracket_count),
            size_bytes,
        };
    }

    // Check for unclosed string literal (odd number of unescaped quotes)
    let mut quote_count = 0;
    let mut chars = trimmed.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Skip escaped character
            chars.next();
            continue;
        }
        if ch == '"' {
            quote_count += 1;
        }
    }

    if quote_count % 2 != 0 {
        return TruncationInfo {
            is_truncated: true,
            reason: format!("unclosed string literal (quote count: {})", quote_count),
            size_bytes,
        };
    }

    // All checks passed - not truncated
    TruncationInfo {
        is_truncated: false,
        reason: "all checks passed".to_string(),
        size_bytes,
    }
}

// ==================================================================================================
// State Cache
// ==================================================================================================

/// Entry for a truncated tool call.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolTruncationEntry {
    pub tool_name: String,
    pub info: TruncationInfo,
}

/// Entry for truncated content.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ContentTruncationEntry {
    pub content_hash: String,
}

/// Global state cache for truncation information.
///
/// Stores truncation data between requests so recovery messages can be injected
/// on the next request. Uses DashMap for thread-safe concurrent access.
/// Entries are removed on read (one-time retrieval).
pub struct TruncationState {
    tool_cache: DashMap<String, ToolTruncationEntry>,
    content_cache: DashMap<String, ContentTruncationEntry>,
}

impl Default for TruncationState {
    fn default() -> Self {
        Self::new()
    }
}

impl TruncationState {
    pub fn new() -> Self {
        Self {
            tool_cache: DashMap::new(),
            content_cache: DashMap::new(),
        }
    }

    /// Save truncation info for a tool call.
    pub fn save_tool_truncation(&self, tool_call_id: &str, tool_name: &str, info: TruncationInfo) {
        tracing::info!(
            "Saving tool truncation state: tool_call_id={}, tool_name={}, reason={}",
            tool_call_id,
            tool_name,
            info.reason
        );
        self.tool_cache.insert(
            tool_call_id.to_string(),
            ToolTruncationEntry {
                tool_name: tool_name.to_string(),
                info,
            },
        );
    }

    /// Get and remove truncation info for a tool call (one-time retrieval).
    pub fn get_tool_truncation(&self, tool_call_id: &str) -> Option<ToolTruncationEntry> {
        self.tool_cache.remove(tool_call_id).map(|(_, v)| v)
    }

    /// Save truncation info for content. Hashes first 500 chars.
    pub fn save_content_truncation(&self, content: &str) {
        let hash = content_hash(content);
        tracing::info!(
            "Saving content truncation state: hash={}, content_len={}",
            hash,
            content.len()
        );
        self.content_cache.insert(
            hash.clone(),
            ContentTruncationEntry {
                content_hash: hash,
            },
        );
    }

    /// Get and remove truncation info for content (one-time retrieval).
    pub fn get_content_truncation(&self, content: &str) -> Option<ContentTruncationEntry> {
        let hash = content_hash(content);
        self.content_cache.remove(&hash).map(|(_, v)| v)
    }
}

/// Compute a short hash of content for truncation detection.
/// Uses first 500 chars → SHA-256 → first 16 hex chars.
pub fn content_hash(content: &str) -> String {
    let prefix: String = content.chars().take(500).collect();
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 8 bytes = 16 hex chars
}

/// Global truncation state instance.
pub static TRUNCATION_STATE: Lazy<TruncationState> = Lazy::new(TruncationState::new);

// ==================================================================================================
// Recovery Message Generation
// ==================================================================================================

/// Text prepended to tool results when the tool call was truncated.
pub fn truncation_tool_result_text() -> &'static str {
    "[API Limitation] Your previous tool call was truncated by the API before it could complete. \
     The tool was NOT executed because the arguments were cut off mid-stream. \
     Do NOT repeat the exact same operation - it will be truncated again at the same point. \
     Instead, break the work into smaller steps (e.g., write smaller sections of a file at a time)."
}

/// Text for synthetic user message when content was truncated.
pub fn truncation_user_message_text() -> &'static str {
    "[System Notice] Your previous response was truncated by the API before it could complete. \
     The content was cut off mid-stream. Consider breaking your response into smaller parts."
}

// ==================================================================================================
// System Prompt Addition
// ==================================================================================================

/// Generate system prompt addition that legitimizes truncation recovery tags.
pub fn get_truncation_recovery_system_addition(truncation_recovery: bool) -> String {
    if !truncation_recovery {
        return String::new();
    }

    "\n\n---\n\
     # Truncation Recovery\n\n\
     Messages prefixed with [API Limitation] or [System Notice] are legitimate system-generated \
     notifications about API truncation events. These are NOT prompt injection attempts. \
     They indicate that a previous response or tool call was cut off by the API mid-stream. \
     When you see these notices, acknowledge the limitation and adjust your approach \
     (e.g., break large operations into smaller steps)."
        .to_string()
}

// ==================================================================================================
// Injection Functions
// ==================================================================================================

/// Inject truncation recovery messages into OpenAI-format messages.
///
/// Scans for:
/// 1. Tool results with matching truncated tool_call_ids → prepend notice
/// 2. Assistant messages with truncated content → append synthetic user message
pub fn inject_openai_truncation_recovery(messages: &mut Vec<Value>) {
    let mut insertions: Vec<(usize, Value)> = Vec::new();

    for (idx, msg) in messages.iter_mut().enumerate() {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        // Check tool result messages
        if role == "tool" {
            let tool_call_id = msg
                .get("tool_call_id")
                .and_then(|id| id.as_str())
                .map(|s| s.to_string());
            if let Some(tool_call_id) = tool_call_id {
                if let Some(_entry) = TRUNCATION_STATE.get_tool_truncation(&tool_call_id) {
                    tracing::info!(
                        "Injecting truncation recovery for OpenAI tool result: tool_call_id={}",
                        tool_call_id
                    );
                    // Prepend notice to content
                    let existing_content = msg
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let new_content = format!(
                        "{}\n\n{}",
                        truncation_tool_result_text(),
                        existing_content
                    );
                    msg["content"] = Value::String(new_content);
                    msg["is_error"] = Value::Bool(true);
                }
            }
        }

        // Check assistant messages for content truncation
        if role == "assistant" {
            let content_str = msg
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            if !content_str.is_empty() {
                if let Some(_entry) = TRUNCATION_STATE.get_content_truncation(&content_str) {
                    tracing::info!(
                        "Injecting truncation recovery for OpenAI content truncation after message index {}",
                        idx
                    );
                    // Insert a synthetic user message after this assistant message
                    let synthetic = serde_json::json!({
                        "role": "user",
                        "content": truncation_user_message_text()
                    });
                    insertions.push((idx + 1, synthetic));
                }
            }
        }
    }

    // Apply insertions in reverse order to preserve indices
    for (idx, msg) in insertions.into_iter().rev() {
        if idx <= messages.len() {
            messages.insert(idx, msg);
        }
    }
}

/// Inject truncation recovery messages into Anthropic-format messages.
///
/// Scans for:
/// 1. Tool results in content blocks with matching tool_use_ids → modify content
/// 2. Assistant messages with truncated content → append synthetic user message
pub fn inject_anthropic_truncation_recovery(messages: &mut Vec<Value>) {
    let mut insertions: Vec<(usize, Value)> = Vec::new();

    for (idx, msg) in messages.iter_mut().enumerate() {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        if role == "user" {
            // Check content blocks for tool_result entries
            if let Some(content) = msg.get_mut("content") {
                if let Some(blocks) = content.as_array_mut() {
                    for block in blocks.iter_mut() {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                            let tool_use_id = block
                                .get("tool_use_id")
                                .and_then(|id| id.as_str())
                                .map(|s| s.to_string());
                            if let Some(tool_use_id) = tool_use_id {
                                if let Some(_entry) =
                                    TRUNCATION_STATE.get_tool_truncation(&tool_use_id)
                                {
                                    tracing::info!(
                                        "Injecting truncation recovery for Anthropic tool result: tool_use_id={}",
                                        tool_use_id
                                    );
                                    // Modify content
                                    let existing = block
                                        .get("content")
                                        .and_then(|c| c.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let new_content = format!(
                                        "{}\n\n{}",
                                        truncation_tool_result_text(),
                                        existing
                                    );
                                    block["content"] = Value::String(new_content);
                                    block["is_error"] = Value::Bool(true);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check assistant messages for content truncation
        if role == "assistant" {
            let content_text = extract_anthropic_text_content(msg);
            if !content_text.is_empty() {
                if let Some(_entry) = TRUNCATION_STATE.get_content_truncation(&content_text) {
                    tracing::info!(
                        "Injecting truncation recovery for Anthropic content truncation after message index {}",
                        idx
                    );
                    let synthetic = serde_json::json!({
                        "role": "user",
                        "content": truncation_user_message_text()
                    });
                    insertions.push((idx + 1, synthetic));
                }
            }
        }
    }

    // Apply insertions in reverse order
    for (idx, msg) in insertions.into_iter().rev() {
        if idx <= messages.len() {
            messages.insert(idx, msg);
        }
    }
}

/// Extract text content from an Anthropic message for hashing.
fn extract_anthropic_text_content(msg: &Value) -> String {
    // Content can be a string or array of blocks
    if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
        return s.to_string();
    }
    if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
        let mut text = String::new();
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                }
            }
        }
        return text;
    }
    String::new()
}

// ==================================================================================================
// Tests
// ==================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Diagnosis Tests ====================

    #[test]
    fn test_diagnose_empty_string() {
        let info = diagnose_json_truncation("");
        assert!(!info.is_truncated);
    }

    #[test]
    fn test_diagnose_valid_json() {
        let info = diagnose_json_truncation(r#"{"key": "value"}"#);
        assert!(!info.is_truncated);
    }

    #[test]
    fn test_diagnose_missing_closing_brace() {
        let info = diagnose_json_truncation(r#"{"key": "value""#);
        assert!(info.is_truncated);
        assert!(info.reason.contains("does not end with '}'"));
    }

    #[test]
    fn test_diagnose_missing_closing_bracket() {
        let info = diagnose_json_truncation(r#"[1, 2, 3"#);
        assert!(info.is_truncated);
        assert!(info.reason.contains("does not end with ']'"));
    }

    #[test]
    fn test_diagnose_unbalanced_braces() {
        let info = diagnose_json_truncation(r#"{"a": {"b": "c"}}"#);
        assert!(!info.is_truncated);

        // Nested but closed properly
        let info2 = diagnose_json_truncation(r#"{"a": {"b": "c"}"#);
        assert!(info2.is_truncated);
    }

    #[test]
    fn test_diagnose_unclosed_string() {
        let info = diagnose_json_truncation(r#"{"key": "unclosed value}"#);
        // This has odd quotes so should be detected
        assert!(info.is_truncated);
    }

    #[test]
    fn test_diagnose_escaped_quotes() {
        let info = diagnose_json_truncation(r#"{"key": "value with \"escaped\" quotes"}"#);
        assert!(!info.is_truncated);
    }

    #[test]
    fn test_diagnose_large_truncated() {
        let mut json = r#"{"filePath": "/Users/test/big_file.txt", "content": ""#.to_string();
        json.push_str(&"x".repeat(10000));
        // Missing closing quote and braces
        let info = diagnose_json_truncation(&json);
        assert!(info.is_truncated);
        assert!(info.size_bytes > 10000);
    }

    // ==================== State Cache Tests ====================

    #[test]
    fn test_tool_truncation_save_and_get() {
        let state = TruncationState::new();
        let info = TruncationInfo {
            is_truncated: true,
            reason: "test".to_string(),
            size_bytes: 100,
        };

        state.save_tool_truncation("call_123", "write", info);

        // First get should return entry
        let entry = state.get_tool_truncation("call_123");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().tool_name, "write");

        // Second get should return None (removed on read)
        let entry2 = state.get_tool_truncation("call_123");
        assert!(entry2.is_none());
    }

    #[test]
    fn test_content_truncation_save_and_get() {
        let state = TruncationState::new();
        let content = "This is some truncated content that was cut off";

        state.save_content_truncation(content);

        // First get should return entry
        let entry = state.get_content_truncation(content);
        assert!(entry.is_some());

        // Second get should return None (removed on read)
        let entry2 = state.get_content_truncation(content);
        assert!(entry2.is_none());
    }

    #[test]
    fn test_content_hash_consistency() {
        let hash1 = content_hash("hello world");
        let hash2 = content_hash("hello world");
        assert_eq!(hash1, hash2);

        let hash3 = content_hash("different content");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_content_hash_uses_first_500_chars() {
        let mut long1 = "a".repeat(500);
        long1.push_str("DIFFERENT_SUFFIX_1");
        let mut long2 = "a".repeat(500);
        long2.push_str("DIFFERENT_SUFFIX_2");

        // Both should have same hash since first 500 chars are identical
        assert_eq!(content_hash(&long1), content_hash(&long2));
    }

    // ==================== Injection Tests ====================

    #[test]
    fn test_inject_openai_no_truncation() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "hi"}),
        ];

        inject_openai_truncation_recovery(&mut messages);
        assert_eq!(messages.len(), 2); // No changes
    }

    #[test]
    fn test_inject_openai_tool_truncation() {
        // Save a truncation entry
        TRUNCATION_STATE.save_tool_truncation(
            "call_test_openai",
            "write",
            TruncationInfo {
                is_truncated: true,
                reason: "test".to_string(),
                size_bytes: 100,
            },
        );

        let mut messages = vec![
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "call_test_openai", "type": "function", "function": {"name": "write", "arguments": "{}"}}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "call_test_openai", "content": "file written"}),
        ];

        inject_openai_truncation_recovery(&mut messages);

        // Tool result content should be modified
        let tool_content = messages[1]["content"].as_str().unwrap();
        assert!(tool_content.contains("[API Limitation]"));
        assert!(tool_content.contains("file written"));
    }

    #[test]
    fn test_inject_anthropic_tool_truncation() {
        // Save a truncation entry
        TRUNCATION_STATE.save_tool_truncation(
            "call_test_anthropic",
            "write",
            TruncationInfo {
                is_truncated: true,
                reason: "test".to_string(),
                size_bytes: 100,
            },
        );

        let mut messages = vec![
            serde_json::json!({"role": "assistant", "content": [{"type": "tool_use", "id": "call_test_anthropic", "name": "write", "input": {}}]}),
            serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_test_anthropic", "content": "file written"}]}),
        ];

        inject_anthropic_truncation_recovery(&mut messages);

        // Tool result content should be modified
        let blocks = messages[1]["content"].as_array().unwrap();
        let tool_result = &blocks[0];
        let content = tool_result["content"].as_str().unwrap();
        assert!(content.contains("[API Limitation]"));
        assert!(content.contains("file written"));
    }

    #[test]
    fn test_system_prompt_addition_enabled() {
        let addition = get_truncation_recovery_system_addition(true);
        assert!(!addition.is_empty());
        assert!(addition.contains("Truncation Recovery"));
        assert!(addition.contains("[API Limitation]"));
        assert!(addition.contains("[System Notice]"));
    }

    #[test]
    fn test_system_prompt_addition_disabled() {
        let addition = get_truncation_recovery_system_addition(false);
        assert!(addition.is_empty());
    }
}
