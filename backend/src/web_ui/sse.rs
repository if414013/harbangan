use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use serde_json::json;
use tokio_stream::StreamExt as _;

use crate::routes::AppState;

/// GET /_ui/api/stream/metrics - SSE stream of metrics snapshots (1s interval)
pub async fn metrics_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(1)))
            .map(move |_| {
                let snapshot = state.metrics.to_json_snapshot();
                let data = serde_json::to_string(&snapshot).unwrap_or_default();
                Ok(Event::default().event("metrics").data(data))
            });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// GET /_ui/api/stream/logs - SSE stream of new log entries (500ms poll)
pub async fn logs_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut last_count: usize = state.log_buffer.lock().map(|b| b.len()).unwrap_or(0);

    let stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
        Duration::from_millis(500),
    ))
    .filter_map(move |_| {
        let buffer = state.log_buffer.lock().ok()?;
        let current = buffer.len();
        if current <= last_count {
            last_count = current;
            return None;
        }
        let new_entries: Vec<serde_json::Value> = buffer
            .iter()
            .skip(last_count)
            .map(|e| {
                json!({
                    "timestamp": e.timestamp.to_rfc3339(),
                    "level": e.level.to_string(),
                    "message": e.message,
                })
            })
            .collect();
        last_count = current;
        Some(Ok(Event::default().event("log").data(
            serde_json::to_string(&new_entries).unwrap_or_default(),
        )))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
