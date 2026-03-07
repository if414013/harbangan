/// Standard SSE (Server-Sent Events) stream parser for direct provider APIs.
///
/// Used by AnthropicProvider, OpenAIProvider, and GeminiProvider.
/// The Kiro provider uses its own AWS Event Stream parser (`streaming/mod.rs`).
use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use crate::error::ApiError;

/// Parse a standard SSE byte stream into a stream of JSON values.
///
/// Handles:
/// - Lines starting with `data: ` (RFC 8895 SSE format)
/// - `data: [DONE]` sentinel — terminates the stream
/// - Lines with no `data: ` prefix are ignored (event:, id:, comment lines)
/// - Multi-line events are not supported (each data: line is a separate event)
///
/// Returns `ApiError::Internal` on JSON parse failure or upstream byte stream errors.
pub fn parse_sse_stream<S>(byte_stream: S) -> impl Stream<Item = Result<Value, ApiError>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    let mut buffer = String::new();

    async_stream::stream! {
        futures::pin_mut!(byte_stream);

        while let Some(chunk) = byte_stream.next().await {
            let chunk = match chunk {
                Ok(b) => b,
                Err(e) => {
                    yield Err(ApiError::Internal(anyhow::anyhow!("SSE stream error: {}", e)));
                    return;
                }
            };

            let text = match std::str::from_utf8(chunk.as_ref()) {
                Ok(s) => s,
                Err(e) => {
                    yield Err(ApiError::Internal(anyhow::anyhow!("SSE invalid UTF-8: {}", e)));
                    return;
                }
            };

            buffer.push_str(text);

            // Process complete lines from the buffer
            loop {
                match buffer.find('\n') {
                    None => break,
                    Some(pos) => {
                        let line = buffer[..pos].trim_end_matches('\r').to_string();
                        buffer = buffer[pos + 1..].to_string();

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                return;
                            }
                            match serde_json::from_str::<Value>(data) {
                                Ok(value) => yield Ok(value),
                                Err(e) => {
                                    tracing::warn!(
                                        data = %data,
                                        error = %e,
                                        "SSE: failed to parse JSON event"
                                    );
                                    // Don't yield error for individual parse failures —
                                    // some providers send non-JSON comment lines
                                }
                            }
                        }
                        // Lines without `data: ` prefix (event:, id:, :comment) are silently skipped
                    }
                }
            }
        }

        // Process any remaining data in the buffer after the stream ends
        for line in buffer.lines() {
            if let Some(data) = line.trim_end_matches('\r').strip_prefix("data: ") {
                if data == "[DONE]" {
                    return;
                }
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    yield Ok(value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn make_stream(data: &'static str) -> impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> {
        let bytes = bytes::Bytes::from_static(data.as_bytes());
        stream::once(async move { Ok::<bytes::Bytes, reqwest::Error>(bytes) })
    }

    fn make_chunked_stream(
        chunks: Vec<&'static str>,
    ) -> impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> {
        stream::iter(
            chunks.into_iter().map(|s| {
                Ok::<bytes::Bytes, reqwest::Error>(bytes::Bytes::from_static(s.as_bytes()))
            }),
        )
    }

    async fn collect_stream(
        s: impl Stream<Item = Result<Value, ApiError>> + Send,
    ) -> Vec<Result<Value, ApiError>> {
        futures::pin_mut!(s);
        let mut results = Vec::new();
        while let Some(item) = s.next().await {
            results.push(item);
        }
        results
    }

    #[tokio::test]
    async fn test_parse_sse_single_event() {
        let data = "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\"}\n\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 1);
        let val = results[0].as_ref().unwrap();
        assert_eq!(val["id"], "1");
        assert_eq!(val["object"], "chat.completion.chunk");
    }

    #[tokio::test]
    async fn test_parse_sse_done_sentinel_terminates_stream() {
        let data = "data: {\"id\":\"1\"}\n\ndata: [DONE]\n\ndata: {\"id\":\"2\"}\n\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        // Only the first event before [DONE]
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap()["id"], "1");
    }

    #[tokio::test]
    async fn test_parse_sse_multiple_events() {
        let data = "data: {\"index\":0}\n\ndata: {\"index\":1}\n\ndata: {\"index\":2}\n\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 3);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.as_ref().unwrap()["index"], i);
        }
    }

    #[tokio::test]
    async fn test_parse_sse_skips_non_data_lines() {
        let data = "event: content_block_delta\nid: 42\ndata: {\"value\":\"hello\"}\n\n: this is a comment\ndata: {\"value\":\"world\"}\n\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap()["value"], "hello");
        assert_eq!(results[1].as_ref().unwrap()["value"], "world");
    }

    #[tokio::test]
    async fn test_parse_sse_empty_stream() {
        let data = "";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_parse_sse_only_done() {
        let data = "data: [DONE]\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_parse_sse_chunked_across_boundaries() {
        // Simulate a data: line split across two chunks
        let chunks = vec!["data: {\"i", "d\":\"split\"}\n\ndata: [DONE]\n"];
        let results = collect_stream(parse_sse_stream(make_chunked_stream(chunks))).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap()["id"], "split");
    }

    #[tokio::test]
    async fn test_parse_sse_invalid_json_skipped() {
        // A non-JSON data line should be skipped (logged as warning), stream continues
        let data = "data: not-json\n\ndata: {\"ok\":true}\n\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        // "not-json" is skipped, only valid JSON event is returned
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap()["ok"], true);
    }

    #[tokio::test]
    async fn test_parse_sse_anthropic_format() {
        let data = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap()["type"], "message_start");
        assert_eq!(results[1].as_ref().unwrap()["type"], "content_block_delta");
        assert_eq!(results[2].as_ref().unwrap()["type"], "message_stop");
    }

    #[tokio::test]
    async fn test_parse_sse_openai_format() {
        let data = concat!(
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" there\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].as_ref().unwrap()["choices"][0]["delta"]["content"],
            "Hi"
        );
        assert_eq!(
            results[1].as_ref().unwrap()["choices"][0]["delta"]["content"],
            " there"
        );
    }

    #[tokio::test]
    async fn test_parse_sse_windows_line_endings() {
        let data = "data: {\"id\":\"cr\"}\r\n\r\ndata: [DONE]\r\n";
        let results = collect_stream(parse_sse_stream(make_stream(data))).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].as_ref().unwrap()["id"], "cr");
    }
}
