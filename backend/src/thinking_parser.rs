/// Parser states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParserState {
    /// Initial state, buffering to detect opening tag
    PreContent,
    /// Inside thinking block, buffering until closing tag
    InThinking,
    /// Regular streaming, no more thinking block detection
    Streaming,
}

/// Result of processing a content chunk through the parser
#[derive(Debug, Default)]
pub struct ThinkingParseResult {
    /// Content to be sent as reasoning_content
    pub thinking_content: Option<String>,
    /// Regular content to be sent as delta.content
    pub regular_content: Option<String>,
    /// True if this is the first chunk of thinking content
    pub is_first_thinking_chunk: bool,
    /// True if thinking block just closed
    pub is_last_thinking_chunk: bool,
    /// True if parser state changed during this feed
    pub state_changed: bool,
}

/// Finite state machine parser for thinking blocks in streaming responses
pub struct ThinkingParser {
    /// How to handle thinking blocks
    pub handling_mode: String,
    /// List of opening tags to detect
    pub open_tags: Vec<String>,
    /// Max chars to buffer while looking for opening tag
    pub initial_buffer_size: usize,
    /// Max tag length for cautious buffering
    pub max_tag_length: usize,

    /// Current state
    pub state: ParserState,
    /// Buffer for initial content (looking for opening tag)
    pub initial_buffer: String,
    /// Buffer for thinking content
    pub thinking_buffer: String,
    /// Detected opening tag
    pub open_tag: Option<String>,
    /// Corresponding closing tag
    pub close_tag: Option<String>,
    /// Whether this is the first thinking chunk
    pub is_first_thinking_chunk: bool,
    /// Whether a thinking block was found
    pub thinking_block_found: bool,
}

impl ThinkingParser {
    /// Create a new thinking parser with default settings
    pub fn new() -> Self {
        let open_tags = vec![
            "<thinking>".to_string(),
            "<think>".to_string(),
            "<reasoning>".to_string(),
            "<thought>".to_string(),
        ];
        let max_tag_length = open_tags.iter().map(|t| t.len()).max().unwrap_or(10) * 2;

        Self {
            handling_mode: "as_reasoning_content".to_string(),
            open_tags,
            initial_buffer_size: 20,
            max_tag_length,
            state: ParserState::PreContent,
            initial_buffer: String::new(),
            thinking_buffer: String::new(),
            open_tag: None,
            close_tag: None,
            is_first_thinking_chunk: true,
            thinking_block_found: false,
        }
    }

    /// Create a new thinking parser with custom settings
    #[allow(dead_code)]
    pub fn with_config(handling_mode: &str, initial_buffer_size: usize) -> Self {
        let mut parser = Self::new();
        parser.handling_mode = handling_mode.to_string();
        parser.initial_buffer_size = initial_buffer_size;
        parser
    }

    /// Process a chunk of content through the parser
    pub fn feed(&mut self, content: &str) -> ThinkingParseResult {
        let mut result = ThinkingParseResult::default();

        if content.is_empty() {
            return result;
        }

        // Handle based on current state
        match self.state {
            ParserState::PreContent => {
                result = self.handle_pre_content(content);

                // If state changed to InThinking, content is already in thinking_buffer
                if self.state == ParserState::InThinking && result.state_changed {
                    // Process thinking buffer for potential closing tag
                    let thinking_result = self.process_thinking_buffer();
                    if thinking_result.thinking_content.is_some() {
                        result.thinking_content = thinking_result.thinking_content;
                        result.is_first_thinking_chunk = thinking_result.is_first_thinking_chunk;
                    }
                    if thinking_result.is_last_thinking_chunk {
                        result.is_last_thinking_chunk = true;
                    }
                    if thinking_result.regular_content.is_some() {
                        result.regular_content = thinking_result.regular_content;
                    }
                }
            }
            ParserState::InThinking => {
                result = self.handle_in_thinking(content);
            }
            ParserState::Streaming => {
                result.regular_content = Some(content.to_string());
            }
        }

        result
    }

    /// Handle content in PreContent state
    fn handle_pre_content(&mut self, content: &str) -> ThinkingParseResult {
        let mut result = ThinkingParseResult::default();
        self.initial_buffer.push_str(content);

        // Strip leading whitespace for tag detection
        let stripped = self.initial_buffer.trim_start();

        // Check if buffer starts with any of the opening tags
        for tag in &self.open_tags.clone() {
            if stripped.starts_with(tag) {
                // Tag found! Transition to InThinking
                self.state = ParserState::InThinking;
                self.open_tag = Some(tag.clone());
                self.close_tag = Some(format!("</{}", &tag[1..]));
                self.thinking_block_found = true;
                result.state_changed = true;

                // Content after the tag goes to thinking buffer
                let content_after_tag = &stripped[tag.len()..];
                self.thinking_buffer = content_after_tag.to_string();
                self.initial_buffer.clear();

                return result;
            }
        }

        // Check if we might still be receiving the tag
        for tag in &self.open_tags {
            if tag.starts_with(stripped) && stripped.len() < tag.len() {
                // Could still be receiving the tag, keep buffering
                return result;
            }
        }

        // No tag found and buffer is either too long or doesn't match any tag prefix
        if self.initial_buffer.len() > self.initial_buffer_size
            || !self.could_be_tag_prefix(stripped)
        {
            // No thinking block, transition to Streaming
            self.state = ParserState::Streaming;
            result.state_changed = true;
            result.regular_content = Some(self.initial_buffer.clone());
            self.initial_buffer.clear();
        }

        result
    }

    /// Check if text could be the start of any opening tag
    fn could_be_tag_prefix(&self, text: &str) -> bool {
        if text.is_empty() {
            return true;
        }

        for tag in &self.open_tags {
            if tag.starts_with(text) {
                return true;
            }
        }
        false
    }

    /// Handle content in InThinking state
    fn handle_in_thinking(&mut self, content: &str) -> ThinkingParseResult {
        self.thinking_buffer.push_str(content);
        self.process_thinking_buffer()
    }

    /// Process the thinking buffer, looking for closing tag
    fn process_thinking_buffer(&mut self) -> ThinkingParseResult {
        let mut result = ThinkingParseResult::default();

        let Some(close_tag) = &self.close_tag.clone() else {
            return result;
        };

        // Check for closing tag
        if let Some(idx) = self.thinking_buffer.find(close_tag) {
            // Found closing tag!
            let thinking_content = self.thinking_buffer[..idx].to_string();
            let after_tag = self.thinking_buffer[idx + close_tag.len()..].to_string();

            // Send all thinking content
            if !thinking_content.is_empty() {
                result.thinking_content = Some(thinking_content);
                result.is_first_thinking_chunk = self.is_first_thinking_chunk;
                self.is_first_thinking_chunk = false;
            }

            result.is_last_thinking_chunk = true;

            // Transition to Streaming
            self.state = ParserState::Streaming;
            result.state_changed = true;
            self.thinking_buffer.clear();

            // Content after closing tag is regular content
            // Strip leading whitespace/newlines that often follow the closing tag
            let stripped_after = after_tag.trim_start();
            if !stripped_after.is_empty() {
                result.regular_content = Some(stripped_after.to_string());
            }

            return result;
        }

        // No closing tag yet - use "cautious" sending
        // Keep last max_tag_length chars in buffer to avoid splitting tag
        if self.thinking_buffer.len() > self.max_tag_length {
            let split_point = self.thinking_buffer.len() - self.max_tag_length;
            // Find a valid UTF-8 character boundary at or before split_point
            let safe_split = self
                .thinking_buffer
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= split_point)
                .last()
                .unwrap_or(0);

            if safe_split > 0 {
                let send_part = self.thinking_buffer[..safe_split].to_string();
                self.thinking_buffer = self.thinking_buffer[safe_split..].to_string();

                result.thinking_content = Some(send_part);
                result.is_first_thinking_chunk = self.is_first_thinking_chunk;
                self.is_first_thinking_chunk = false;
            }
        }

        result
    }

    /// Finalize parsing when stream ends
    #[allow(dead_code)]
    pub fn finalize(&mut self) -> ThinkingParseResult {
        let mut result = ThinkingParseResult::default();

        // Flush thinking buffer if we're still in thinking state
        if !self.thinking_buffer.is_empty() {
            if self.state == ParserState::InThinking {
                result.thinking_content = Some(self.thinking_buffer.clone());
                result.is_first_thinking_chunk = self.is_first_thinking_chunk;
                result.is_last_thinking_chunk = true;
            } else {
                result.regular_content = Some(self.thinking_buffer.clone());
            }
            self.thinking_buffer.clear();
        }

        // Flush initial buffer if we never found a tag
        if !self.initial_buffer.is_empty() {
            let existing = result.regular_content.unwrap_or_default();
            result.regular_content = Some(existing + &self.initial_buffer);
            self.initial_buffer.clear();
        }

        result
    }

    /// Reset parser to initial state
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.state = ParserState::PreContent;
        self.initial_buffer.clear();
        self.thinking_buffer.clear();
        self.open_tag = None;
        self.close_tag = None;
        self.is_first_thinking_chunk = true;
        self.thinking_block_found = false;
    }

    /// Process thinking content according to handling mode
    pub fn process_for_output(
        &self,
        thinking_content: &str,
        is_first: bool,
        is_last: bool,
    ) -> Option<String> {
        if thinking_content.is_empty() {
            return None;
        }

        match self.handling_mode.as_str() {
            "remove" => None,
            "pass" => {
                // Add tags back
                let prefix = if is_first {
                    self.open_tag.as_deref().unwrap_or("")
                } else {
                    ""
                };
                let suffix = if is_last {
                    self.close_tag.as_deref().unwrap_or("")
                } else {
                    ""
                };
                Some(format!("{}{}{}", prefix, thinking_content, suffix))
            }
            "strip_tags" => Some(thinking_content.to_string()),
            _ => Some(thinking_content.to_string()), // "as_reasoning_content" - return as-is
        }
    }
}

impl Default for ThinkingParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_thinking_block() {
        let mut parser = ThinkingParser::new();

        // Feed thinking tag
        let result = parser.feed("<thinking>Hello world</thinking>Done");

        assert!(result.thinking_content.is_some());
        assert!(result.regular_content.is_some());
        assert_eq!(result.regular_content.unwrap(), "Done");
    }

    #[test]
    fn test_no_thinking_block() {
        let mut parser = ThinkingParser::new();

        // Feed content without thinking tag
        let result = parser.feed("Hello world, this is regular content");

        assert!(result.thinking_content.is_none());
        assert!(result.regular_content.is_some());
    }

    #[test]
    fn test_split_tag() {
        let mut parser = ThinkingParser::new();

        // Feed partial tag
        let result1 = parser.feed("<think");
        assert!(result1.thinking_content.is_none());
        assert!(result1.regular_content.is_none());

        // Complete the tag
        let _result2 = parser.feed("ing>Hello");
        assert_eq!(parser.state, ParserState::InThinking);
    }

    #[test]
    fn test_think_tag_variant() {
        let mut parser = ThinkingParser::new();

        let result = parser.feed("<think>My thoughts</think>Response");

        assert!(result.thinking_content.is_some());
        assert!(result.regular_content.is_some());
        assert_eq!(result.regular_content.unwrap(), "Response");
    }

    #[test]
    fn test_reasoning_tag_variant() {
        let mut parser = ThinkingParser::new();

        let result = parser.feed("<reasoning>Analysis here</reasoning>Final answer");

        assert!(result.thinking_content.is_some());
        assert!(result.regular_content.is_some());
        assert_eq!(result.regular_content.unwrap(), "Final answer");
    }

    #[test]
    fn test_thought_tag_variant() {
        let mut parser = ThinkingParser::new();

        let result = parser.feed("<thought>Internal thought</thought>Output");

        assert!(result.thinking_content.is_some());
        assert!(result.regular_content.is_some());
    }

    #[test]
    fn test_whitespace_before_tag() {
        let mut parser = ThinkingParser::new();

        // Whitespace before tag should be stripped for detection
        let result = parser.feed("   <thinking>Content</thinking>Done");

        assert!(result.thinking_content.is_some());
        assert_eq!(parser.thinking_block_found, true);
    }

    #[test]
    fn test_streaming_thinking_content() {
        let mut parser = ThinkingParser::new();

        // First chunk - start of thinking
        let _result1 = parser.feed("<thinking>First part");
        assert_eq!(parser.state, ParserState::InThinking);

        // Second chunk - more thinking content
        let _result2 = parser.feed(" second part");
        // Content may be buffered due to cautious sending

        // Third chunk - end of thinking
        let result3 = parser.feed("</thinking>Regular content");
        assert!(result3.is_last_thinking_chunk);
        assert!(result3.regular_content.is_some());
        assert_eq!(result3.regular_content.unwrap(), "Regular content");
    }

    #[test]
    fn test_finalize_in_thinking_state() {
        let mut parser = ThinkingParser::new();

        // Start thinking but don't close it
        parser.feed("<thinking>Incomplete thinking");
        assert_eq!(parser.state, ParserState::InThinking);

        // Finalize should flush the buffer
        let result = parser.finalize();
        assert!(result.thinking_content.is_some());
        assert!(result.is_last_thinking_chunk);
    }

    #[test]
    fn test_finalize_in_pre_content_state() {
        let mut parser = ThinkingParser::new();

        // Feed partial content that could be a tag
        parser.feed("<thin");
        assert_eq!(parser.state, ParserState::PreContent);

        // Finalize should flush as regular content
        let result = parser.finalize();
        assert!(result.regular_content.is_some());
    }

    #[test]
    fn test_reset() {
        let mut parser = ThinkingParser::new();

        // Process some content
        parser.feed("<thinking>Content</thinking>Done");
        assert!(parser.thinking_block_found);

        // Reset
        parser.reset();

        assert_eq!(parser.state, ParserState::PreContent);
        assert!(!parser.thinking_block_found);
        assert!(parser.initial_buffer.is_empty());
        assert!(parser.thinking_buffer.is_empty());
    }

    #[test]
    fn test_process_for_output_remove_mode() {
        let mut parser = ThinkingParser::new();
        parser.handling_mode = "remove".to_string();

        let result = parser.process_for_output("thinking content", true, false);
        assert!(result.is_none());
    }

    #[test]
    fn test_process_for_output_pass_mode() {
        let mut parser = ThinkingParser::new();
        parser.handling_mode = "pass".to_string();
        parser.open_tag = Some("<thinking>".to_string());
        parser.close_tag = Some("</thinking>".to_string());

        // First chunk
        let result = parser.process_for_output("content", true, false);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("<thinking>"));

        // Last chunk
        let result = parser.process_for_output("more", false, true);
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("</thinking>"));
    }

    #[test]
    fn test_process_for_output_strip_tags_mode() {
        let mut parser = ThinkingParser::new();
        parser.handling_mode = "strip_tags".to_string();

        let result = parser.process_for_output("thinking content", true, true);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "thinking content");
    }

    #[test]
    fn test_process_for_output_as_reasoning_content_mode() {
        let mut parser = ThinkingParser::new();
        parser.handling_mode = "as_reasoning_content".to_string();

        let result = parser.process_for_output("thinking content", true, true);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "thinking content");
    }

    #[test]
    fn test_process_for_output_empty_content() {
        let parser = ThinkingParser::new();

        let result = parser.process_for_output("", true, true);
        assert!(result.is_none());
    }

    #[test]
    fn test_default_trait() {
        let parser = ThinkingParser::default();
        assert_eq!(parser.state, ParserState::PreContent);
        assert!(parser.open_tags.contains(&"<thinking>".to_string()));
    }

    #[test]
    fn test_with_config() {
        let parser = ThinkingParser::with_config("remove", 50);
        assert_eq!(parser.handling_mode, "remove");
        assert_eq!(parser.initial_buffer_size, 50);
    }

    #[test]
    fn test_long_content_without_tag() {
        let mut parser = ThinkingParser::new();

        // Content longer than initial_buffer_size without a tag
        let long_content = "This is a very long content that definitely does not start with any thinking tag and should be treated as regular content immediately.";
        let result = parser.feed(long_content);

        assert_eq!(parser.state, ParserState::Streaming);
        assert!(result.regular_content.is_some());
    }

    #[test]
    fn test_cautious_buffering() {
        let mut parser = ThinkingParser::new();

        // Start thinking block
        parser.feed("<thinking>");

        // Feed content that's longer than max_tag_length
        // This should trigger cautious sending
        let long_thinking = "A".repeat(100);
        let result = parser.feed(&long_thinking);

        // Some content should be sent, but buffer should retain max_tag_length chars
        if result.thinking_content.is_some() {
            assert!(result.thinking_content.unwrap().len() < 100);
        }
    }

    #[test]
    fn test_state_changed_flag() {
        let mut parser = ThinkingParser::new();

        // Transition from PreContent to InThinking
        let result = parser.feed("<thinking>content");
        assert!(result.state_changed);

        // No state change within InThinking
        let result2 = parser.feed("more content");
        assert!(!result2.state_changed);

        // Transition from InThinking to Streaming
        let result3 = parser.feed("</thinking>done");
        assert!(result3.state_changed);
    }

    #[test]
    fn test_is_first_thinking_chunk_flag() {
        let mut parser = ThinkingParser::new();

        // First thinking chunk
        parser.feed("<thinking>");
        let result = parser.feed("A".repeat(100).as_str());

        if result.thinking_content.is_some() {
            assert!(result.is_first_thinking_chunk);
        }

        // Subsequent chunks should not be first
        let result2 = parser.feed("B".repeat(100).as_str());
        if result2.thinking_content.is_some() {
            assert!(!result2.is_first_thinking_chunk);
        }
    }

    #[test]
    fn test_empty_feed() {
        let mut parser = ThinkingParser::new();

        let result = parser.feed("");

        assert!(result.thinking_content.is_none());
        assert!(result.regular_content.is_none());
        assert!(!result.state_changed);
    }

    #[test]
    fn test_newline_after_closing_tag() {
        let mut parser = ThinkingParser::new();

        // Newlines after closing tag should be stripped
        let result = parser.feed("<thinking>thought</thinking>\n\nResponse");

        assert!(result.regular_content.is_some());
        assert_eq!(result.regular_content.unwrap(), "Response");
    }
}
