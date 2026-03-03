use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// A captured log entry for the web UI live-log stream.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: tracing::Level,
    pub message: String,
}

/// Maximum number of log entries to keep in buffer.
const DEFAULT_BUFFER_CAPACITY: usize = 1000;

/// Custom tracing layer that captures log events to a shared buffer
/// so the web UI can stream them via SSE.
pub struct LogCaptureLayer {
    buffer: Arc<Mutex<VecDeque<LogEntry>>>,
    capacity: usize,
    min_level: Level,
}

impl LogCaptureLayer {
    pub fn new(buffer: Arc<Mutex<VecDeque<LogEntry>>>) -> Self {
        Self {
            buffer,
            capacity: DEFAULT_BUFFER_CAPACITY,
            min_level: Level::INFO,
        }
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        if metadata.level() > &self.min_level {
            return;
        }

        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono::Utc::now(),
            level: *metadata.level(),
            message,
        };

        if let Ok(mut buffer) = self.buffer.try_lock() {
            while buffer.len() >= self.capacity {
                buffer.pop_front();
            }
            buffer.push_back(entry);
        }
    }
}

/// Visitor to extract message field from tracing events.
struct MessageVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            *self.0 = format!("{:?}", value);
            if self.0.starts_with('"') && self.0.ends_with('"') {
                *self.0 = self.0[1..self.0.len() - 1].to_string();
            }
        } else if self.0.is_empty() {
            *self.0 = format!("{}: {:?}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            *self.0 = value.to_string();
        } else if self.0.is_empty() {
            *self.0 = format!("{}: {}", field.name(), value);
        } else {
            self.0.push_str(&format!(" {}={}", field.name(), value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn test_log_capture_layer_captures_info() {
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let layer = LogCaptureLayer::new(Arc::clone(&buffer));

        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("Test info message");
        });

        let logs = buffer.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].message.contains("Test info message"));
    }

    #[test]
    fn test_log_capture_layer_filters_debug() {
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let layer = LogCaptureLayer::new(Arc::clone(&buffer));

        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("Debug message should be filtered");
        });

        let logs = buffer.lock().unwrap();
        assert_eq!(logs.len(), 0);
    }

    #[test]
    fn test_buffer_capacity_limit() {
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let mut layer = LogCaptureLayer::new(Arc::clone(&buffer));
        layer.capacity = 5;

        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            for i in 0..10 {
                tracing::info!("Message {}", i);
            }
        });

        let logs = buffer.lock().unwrap();
        assert_eq!(logs.len(), 5);
    }
}
