---
layout: default
title: Streaming & Event Parsing
parent: Architecture
nav_order: 3
permalink: /architecture/streaming/
---

# Streaming and Event Parsing
{: .no_toc }

The Kiro API returns responses as a text-based stream containing JSON event objects. This page covers how the gateway parses that stream, extracts meaningful events, processes thinking blocks, detects truncation, and formats the output as Server-Sent Events (SSE) for OpenAI and Anthropic clients.

This streaming pipeline is specific to the Kiro provider path. When requests are routed to direct providers (Anthropic, OpenAI Codex, Copilot, Qwen, Custom) via the `ProviderRegistry`, the provider's `stream_openai()` / `stream_anthropic()` trait methods handle streaming natively — the response SSE stream is parsed by `streaming/sse.rs` and relayed to the client. Cross-format streaming (e.g., OpenAI SSE → Anthropic events) is handled by `streaming/cross_format.rs`.
{: .note }

## Table of Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Streaming Pipeline Overview

```mermaid
flowchart TD
    subgraph Input["Kiro API Response"]
        RAW["Raw HTTP response body<br/><i>Content-Type: application/vnd.amazon.eventstream</i>"]
    end

    subgraph TextParsing["SseParser — Text-Based JSON Extraction"]
        RAW --> CHUNK["Read bytes from stream"]
        CHUNK --> BUFFER["Append to text buffer"]
        BUFFER --> PATTERN["Pattern match against<br/>known JSON prefixes:<br/>{\"content\":, {\"name\":,<br/>{\"usage\":, {\"stop\":, etc."]
        PATTERN --> BRACE["Find matching closing<br/>brace (brace counting)"]
        BRACE --> PARSE_JSON["Parse extracted JSON"]
    end

    subgraph EventExtraction["Event Extraction"]
        PARSE_JSON --> TYPE{"JSON field?"}
        TYPE -->|"content"| KIRO_CONTENT["KiroEvent (content)"]
        TYPE -->|"name/input"| KIRO_TOOL["KiroEvent (tool_use)"]
        TYPE -->|"usage"| KIRO_USAGE["KiroEvent (usage)"]
        TYPE -->|"stop"| KIRO_STOP["Stream complete"]
        KIRO_CONTENT --> KIRO_EVENT["Build KiroEvent"]
        KIRO_TOOL --> KIRO_EVENT
        KIRO_USAGE --> KIRO_EVENT
    end

    subgraph ThinkingProcessing["Thinking Block Processing"]
        KIRO_EVENT --> HAS_CONTENT{"Has text content?"}
        HAS_CONTENT -->|Yes| FEED_FSM["ThinkingParser.feed(content)"]
        HAS_CONTENT -->|No| PASS_THROUGH["Pass event through"]
        FEED_FSM --> RESULT["ThinkingParseResult"]
        RESULT --> THINKING_OUT["thinking_content → reasoning"]
        RESULT --> REGULAR_OUT["regular_content → text"]
    end

    subgraph OutputFormatting["Output Formatting"]
        THINKING_OUT --> FORMAT_OAI["OpenAI SSE<br/><i>reasoning_content in delta</i>"]
        REGULAR_OUT --> FORMAT_OAI
        THINKING_OUT --> FORMAT_ANT["Anthropic SSE<br/><i>thinking content block</i>"]
        REGULAR_OUT --> FORMAT_ANT
        PASS_THROUGH --> FORMAT_OAI
        PASS_THROUGH --> FORMAT_ANT
    end

    subgraph Delivery["Client Delivery"]
        FORMAT_OAI --> CLIENT_OAI["OpenAI Client<br/><i>data: {...}\\n\\n</i>"]
        FORMAT_ANT --> CLIENT_ANT["Anthropic Client<br/><i>event: ...\\ndata: {...}\\n\\n</i>"]
    end
```

---

## SseParser — Text-Based Stream Parsing

The Kiro API streams responses as a text stream containing embedded JSON objects. The `SseParser` (`src/streaming/mod.rs`) extracts events using pattern matching and brace counting — not binary frame parsing.

### How It Works

1. **Buffer accumulation**: Raw bytes are converted to text and appended to an internal string buffer
2. **Pattern matching**: The parser scans the buffer for known JSON prefixes in priority order:
   - `{"content":` — text content
   - `{"name":` — tool use name
   - `{"input":` — tool use arguments
   - `{"stop":` — stream end signal
   - `{"followupPrompt":` — follow-up prompt
   - `{"usage":` — token usage stats
   - `{"contextUsagePercentage":` — context window usage
3. **Brace counting**: Starting from the matched position, `find_matching_brace()` counts `{`/`}` pairs to find the complete JSON object boundary
4. **JSON extraction**: The complete JSON string is parsed with `serde_json` and yielded as an event
5. **Buffer cleanup**: Consumed bytes are removed from the buffer; remaining bytes await more data

### Tool Call Accumulation

Tool calls arrive as separate `name` and `input` events. The `ToolCallAccumulator` collects fragments and assembles complete `ToolUse` objects with `tool_use_id`, `name`, and `input` fields.

### Direct Provider Streaming

For non-Kiro providers, `streaming/sse.rs` provides a standard SSE parser (`parse_sse_stream()`) that handles `data: {json}\n\n` format, including the `data: [DONE]` sentinel. `streaming/cross_format.rs` handles cross-format translation (e.g., OpenAI SSE chunks → Anthropic stream events).

---

## KiroEvent Variants

After extracting JSON events from the stream, they are converted into `KiroEvent` structs. The `event_type` field determines what data the event carries:

| Event Type | Description | Key Fields |
|-----------|-------------|------------|
| `content` | Text content chunk | `content: String` |
| `thinking` | Reasoning/thinking content | `thinking_content: String` |
| `tool_use` | Tool call from the model | `tool_use: {tool_use_id, name, input}` |
| `usage` | Token usage statistics | `usage: {input_tokens, output_tokens}` |
| `context_usage` | Context window utilization | `context_usage_percentage: f64` |

```mermaid
classDiagram
    class KiroEvent {
        +String event_type
        +Option~String~ content
        +Option~String~ thinking_content
        +Option~ToolUse~ tool_use
        +Option~Usage~ usage
        +Option~f64~ context_usage_percentage
        +bool is_first_thinking_chunk
        +bool is_last_thinking_chunk
    }

    class ToolUse {
        +String tool_use_id
        +String name
        +Value input
        +Option~TruncationInfo~ truncation_info
    }

    class Usage {
        +i32 input_tokens
        +i32 output_tokens
    }

    KiroEvent --> ToolUse
    KiroEvent --> Usage
```

---

## Thinking Block Extraction

The `ThinkingParser` (`src/thinking_parser.rs`) is a finite state machine that detects and extracts `<thinking>` blocks from streaming content. This is critical for supporting extended thinking / chain-of-thought reasoning in models that emit their reasoning wrapped in XML-like tags.

### State Machine

```mermaid
stateDiagram-v2
    [*] --> PreContent: Stream starts

    PreContent --> PreContent: Buffering<br/>(might be tag prefix)
    PreContent --> InThinking: Found opening tag<br/>(&lt;thinking&gt;, &lt;think&gt;, etc.)
    PreContent --> Streaming: No tag found<br/>(buffer exceeds limit)

    InThinking --> InThinking: Accumulating thinking content<br/>(cautious buffering)
    InThinking --> Streaming: Found closing tag<br/>(&lt;/thinking&gt;, etc.)

    Streaming --> Streaming: Pass through all content
    Streaming --> [*]: Stream ends
```

### How It Works

1. **PreContent state**: The parser buffers the first ~20 characters of content, looking for an opening tag. It checks against four supported tag variants:
   - `<thinking>...</thinking>`
   - `<think>...</think>`
   - `<reasoning>...</reasoning>`
   - `<thought>...</thought>`

2. **Tag detection**: If the buffer starts with any of these tags (after stripping leading whitespace), the parser transitions to `InThinking` and records which tag was found. The corresponding closing tag is computed automatically (e.g., `<thinking>` → `</thinking>`).

3. **InThinking state**: Content is accumulated in a thinking buffer. The parser uses "cautious buffering" — it keeps the last `max_tag_length` characters in the buffer to avoid accidentally splitting a closing tag across chunks. Content before that safety margin is emitted as `thinking_content`.

4. **Closing tag detection**: When the closing tag is found, the parser transitions to `Streaming`. Content before the closing tag is emitted as the final thinking chunk. Content after the closing tag is emitted as regular content.

5. **Streaming state**: All subsequent content passes through as `regular_content` with no further processing.

### Handling Modes

The `ThinkingParser` supports four handling modes, configured via `fake_reasoning_handling` in the Config:

| Mode | Behavior |
|------|----------|
| `as_reasoning_content` | Extract thinking content to a separate `reasoning_content` field (default, OpenAI-compatible) |
| `remove` | Strip thinking blocks entirely from the output |
| `pass` | Keep the original `<thinking>` tags in the output content |
| `strip_tags` | Remove the tags but keep the thinking content inline |

### Integration with Streaming

The thinking parser is instantiated per-request inside the streaming functions. For each content chunk from the Kiro API:

```mermaid
flowchart LR
    CHUNK["Content chunk<br/>from Kiro"] --> FEED["parser.feed(chunk)"]
    FEED --> RESULT["ThinkingParseResult"]
    RESULT --> TC{"thinking_content?"}
    TC -->|Some| EMIT_T["Emit as reasoning<br/>(OpenAI) or thinking<br/>block (Anthropic)"]
    TC -->|None| CHECK_R{"regular_content?"}
    CHECK_R -->|Some| EMIT_R["Emit as text content"]
    CHECK_R -->|None| SKIP["Skip (still buffering)"]
```

---

## SSE Output Formatting

### OpenAI Format

Each streaming event is formatted as:
```
data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":...,"model":"...","choices":[{"index":0,"delta":{"content":"..."},"finish_reason":null}]}\n\n
```

For thinking content (when `fake_reasoning_enabled` is true):
```
data: {"id":"chatcmpl-...","choices":[{"index":0,"delta":{"reasoning_content":"..."},"finish_reason":null}]}\n\n
```

Stream termination:
```
data: [DONE]\n\n
```

Usage (when `include_usage` is true, sent as the final chunk before `[DONE]`):
```
data: {"id":"chatcmpl-...","choices":[],"usage":{"prompt_tokens":...,"completion_tokens":...,"total_tokens":...}}\n\n
```

### Anthropic Format

Anthropic uses named event types:
```
event: message_start
data: {"type":"message_start","message":{"id":"msg-...","type":"message","role":"assistant","model":"...","content":[],"usage":{"input_tokens":...}}}\n\n

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"..."}}\n\n

event: content_block_stop
data: {"type":"content_block_stop","index":0}\n\n

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":...}}\n\n

event: message_stop
data: {"type":"message_stop"}\n\n
```

For thinking content, a separate content block with type `thinking` is emitted before the text content block.

---

## Truncation Detection and Recovery

The Kiro API can silently truncate large responses mid-stream, particularly tool call arguments. The truncation system (`src/truncation.rs`) provides detection and recovery.

### Detection

The `diagnose_json_truncation()` function uses heuristic analysis on raw JSON strings:

```mermaid
flowchart TD
    INPUT["Raw JSON string"] --> EMPTY{"Empty?"}
    EMPTY -->|Yes| NOT_TRUNC["Not truncated"]
    EMPTY -->|No| BRACE_CHECK{"Starts with '{',<br/>ends with '}'?"}
    BRACE_CHECK -->|No match| TRUNC_1["Truncated:<br/>mismatched delimiters"]
    BRACE_CHECK -->|Match| BRACKET_CHECK{"Starts with '[',<br/>ends with ']'?"}
    BRACKET_CHECK -->|No match| TRUNC_2["Truncated:<br/>mismatched brackets"]
    BRACKET_CHECK -->|Match| COUNT["Count { } [ ] pairs"]
    COUNT --> BALANCED{"Balanced?"}
    BALANCED -->|No| TRUNC_3["Truncated:<br/>unbalanced delimiters"]
    BALANCED -->|Yes| NOT_TRUNC
```

### Recovery

When truncation is detected in a tool call's JSON arguments:

1. A `TruncationInfo` is attached to the `ToolUse` event
2. The truncation state is cached globally (keyed by a hash of the truncated content)
3. On the next request, `inject_openai_truncation_recovery()` or `inject_anthropic_truncation_recovery()` checks the cache and injects a recovery message asking the model to re-emit the truncated content

This creates a self-healing loop: truncated responses are detected, and the next request automatically includes context about what was lost.

---

## Non-Streaming Response Collection

For non-streaming requests, the gateway still receives a stream from the Kiro API. The `collect_openai_response()` and `collect_anthropic_response()` functions consume the entire stream and aggregate it:

```mermaid
flowchart LR
    STREAM["AWS Event Stream"] --> COLLECT["Collect all KiroEvents"]
    COLLECT --> AGG["Aggregate:<br/>- Concatenate content<br/>- Collect tool calls<br/>- Deduplicate tools<br/>- Sum usage tokens"]
    AGG --> BUILD["Build single JSON response"]
    BUILD --> RETURN["Return as JSON body<br/>(not SSE)"]
```

Tool call deduplication (`deduplicate_tool_calls()`) handles a quirk of the Kiro API where the same tool call may appear multiple times in the stream. Deduplication works by:
1. Grouping by `tool_use_id` — keeping the version with the most complete arguments
2. Grouping by `name + arguments` — removing exact duplicates

---

## Key Streaming Functions

| Function | Source | Description |
|----------|--------|-------------|
| `stream_kiro_to_openai()` | `streaming/mod.rs` | Convert Kiro stream to OpenAI SSE format |
| `stream_kiro_to_anthropic()` | `streaming/mod.rs` | Convert Kiro stream to Anthropic SSE format |
| `collect_openai_response()` | `streaming/mod.rs` | Aggregate stream into single OpenAI JSON response |
| `collect_anthropic_response()` | `streaming/mod.rs` | Aggregate stream into single Anthropic JSON response |
| `parse_kiro_stream()` | `streaming/mod.rs` | Parse Kiro text stream into KiroEvents via SseParser |
| `parse_sse_stream()` | `streaming/sse.rs` | Parse standard SSE format for direct providers |
| `OpenAIToAnthropicState` | `streaming/cross_format.rs` | Translate OpenAI SSE chunks to Anthropic stream events |
| `deduplicate_tool_calls()` | `streaming/mod.rs` | Remove duplicate tool calls from collected stream |
| `ThinkingParser::feed()` | `thinking_parser.rs` | Process a content chunk through the thinking FSM |
| `ThinkingParser::finalize()` | `thinking_parser.rs` | Flush remaining buffers when stream ends |
| `diagnose_json_truncation()` | `truncation.rs` | Heuristic truncation detection on JSON strings |

---

## Timeout Handling

The streaming pipeline uses a `first_token_timeout` (default: 15 seconds) to detect stalled streams. If no data arrives within this window after the request is sent, the stream is aborted and an error is returned to the client. This prevents requests from hanging indefinitely when the Kiro API is unresponsive.

For ongoing streams, individual chunk timeouts are not enforced — once the first token arrives, the stream is allowed to complete at whatever pace the API delivers content.
