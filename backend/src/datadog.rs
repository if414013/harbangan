//! Datadog APM tracing and metrics integration.
//!
//! This module initialises the Datadog OpenTelemetry pipelines **only** when the
//! `DD_AGENT_HOST` environment variable is set.  When the variable is absent the
//! functions return `None` and no Datadog code runs at runtime.
//!
//! # Tracing
//!
//! Call [`init_datadog`] before the `tracing_subscriber` registry is built and
//! pass the returned `Option<Layer>` directly to `.with(dd_layer)`.
//! The `Layer for Option<L>` blanket impl in `tracing-subscriber` turns a `None`
//! into a zero-cost no-op.
//!
//! # JSON log-trace correlation
//!
//! When the JSON formatter is active (i.e. Datadog is configured), use
//! [`DdJsonFormat`] as the event formatter.  It wraps the standard JSON
//! formatter and injects `dd.trace_id` and `dd.span_id` as top-level fields
//! on every log line so the Datadog Agent can correlate logs with APM traces.
//!
//! Datadog expects:
//! - `dd.trace_id`: lower 64 bits of the 128-bit OTel trace ID, as a decimal string
//! - `dd.span_id`: the 64-bit OTel span ID, as a decimal string
//!
//! # Metrics
//!
//! Call [`init_otel_metrics`] to initialise a `SdkMeterProvider` that exports
//! metrics via OTLP-HTTP to the Datadog Agent.  When `Some` is returned, the
//! global OTel meter provider is already set — use
//! `opentelemetry::global::meter("harbangan")` to obtain a `Meter`.
//!
//! # Shutdown
//!
//! Call [`shutdown`] (with the optional metrics provider) after the server has
//! stopped to flush any buffered spans and metrics.
//!
//! # Environment variables
//!
//! | Variable          | Default        | Description                              |
//! |-------------------|----------------|------------------------------------------|
//! | `DD_AGENT_HOST`   | *unset* (skip) | Datadog Agent hostname/IP                |
//! | `DD_AGENT_PORT`   | `8126`         | Datadog Agent trace port                 |
//! | `DD_OTLP_PORT`    | `4318`         | Datadog Agent OTLP HTTP port             |
//! | `DD_SERVICE`      | `harbangan`    | APM service name                         |
//! | `DD_ENV`          | *unset*        | Deployment environment tag               |
//! | `DD_VERSION`      | *unset*        | Service version tag                      |

use anyhow::Context as _;
use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

// ── Trace ID conversion ──────────────────────────────────────────────────────

/// Convert a 128-bit OpenTelemetry trace ID to Datadog format.
///
/// Datadog uses the **lower 64 bits** of the 128-bit trace ID, represented as a
/// decimal string.  Returns `"0"` for invalid/zero trace IDs.
pub fn otel_trace_id_to_dd(trace_id: opentelemetry::trace::TraceId) -> String {
    let bytes = trace_id.to_bytes();
    // Lower 64 bits = bytes[8..16]
    let lower_64 = u64::from_be_bytes([
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    lower_64.to_string()
}

/// Convert a 64-bit OpenTelemetry span ID to Datadog format (decimal string).
///
/// Returns `"0"` for invalid/zero span IDs.
pub fn otel_span_id_to_dd(span_id: opentelemetry::trace::SpanId) -> String {
    let bytes = span_id.to_bytes();
    let val = u64::from_be_bytes(bytes);
    val.to_string()
}

// ── DD context middleware ─────────────────────────────────────────────────────

/// Axum middleware that records `dd.trace_id` and `dd.span_id` on the current
/// tracing span.  The `http_request` span must declare these as `Empty` fields.
///
/// This runs in normal async code (not inside a subscriber callback), so
/// accessing the OTel span context via `tracing::Span::current()` is safe.
pub async fn dd_context_middleware(request: Request<Body>, next: Next) -> Response {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    let otel_cx = tracing::Span::current().context();
    let span_ref = otel_cx.span();
    let span_ctx = span_ref.span_context();

    if span_ctx.is_valid() {
        let current = tracing::Span::current();
        current.record(
            "dd.trace_id",
            otel_trace_id_to_dd(span_ctx.trace_id()).as_str(),
        );
        current.record(
            "dd.span_id",
            otel_span_id_to_dd(span_ctx.span_id()).as_str(),
        );
    }

    next.run(request).await
}

// ── Config helpers ────────────────────────────────────────────────────────────

/// Returns `true` when `DD_AGENT_HOST` is present in the environment.
///
/// Use this before the tracing subscriber is initialised (where `tracing::` macros
/// cannot be called yet) to drive conditional log format selection and similar
/// one-time configuration.
pub fn dd_agent_configured() -> bool {
    std::env::var("DD_AGENT_HOST").is_ok()
}

// ── Tracing ──────────────────────────────────────────────────────────────────

/// Initialise the Datadog APM tracing layer.
///
/// Returns `Some(layer)` when `DD_AGENT_HOST` is set and the pipeline
/// initialises successfully; `None` otherwise (zero overhead).
pub fn init_datadog<S>() -> Option<OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let agent_host = std::env::var("DD_AGENT_HOST").ok()?;
    // Validate host: reject values containing URL-special chars that could enable SSRF
    if agent_host.contains('@') || agent_host.contains('/') || agent_host.contains("://") {
        // eprintln! intentional: tracing subscriber is not yet initialized at this call site
        eprintln!("[WARN] DD_AGENT_HOST contains invalid characters — Datadog tracing disabled");
        return None;
    }
    let agent_port: u16 = std::env::var("DD_AGENT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .filter(|&p: &u16| p > 0)
        .unwrap_or(8126);
    let service_name = std::env::var("DD_SERVICE").unwrap_or_else(|_| "harbangan".to_string());
    let agent_endpoint = format!("http://{agent_host}:{agent_port}");

    match build_trace_pipeline(&service_name, &agent_endpoint) {
        Ok(layer) => {
            // eprintln! intentional: tracing subscriber is not yet initialized at this call site
            eprintln!(
                "[INFO] Datadog APM tracing enabled: endpoint={agent_endpoint} service={service_name}"
            );
            Some(layer)
        }
        Err(e) => {
            // eprintln! intentional: tracing subscriber is not yet initialized at this call site
            eprintln!("[WARN] Datadog APM init failed ({e:#}) — tracing disabled");
            None
        }
    }
}

fn build_trace_pipeline<S>(
    service_name: &str,
    agent_endpoint: &str,
) -> anyhow::Result<OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let provider = opentelemetry_datadog::new_pipeline()
        .with_service_name(service_name)
        .with_agent_endpoint(agent_endpoint)
        .with_api_version(opentelemetry_datadog::ApiVersion::Version05)
        .install_batch()
        .context("Failed to install Datadog tracing pipeline")?;

    let tracer = provider.tracer("harbangan");
    opentelemetry::global::set_tracer_provider(provider);

    Ok(tracing_opentelemetry::layer().with_tracer(tracer))
}

// ── Metrics ───────────────────────────────────────────────────────────────────

/// Initialise the OTLP metrics pipeline targeting the Datadog Agent.
///
/// Returns `Some(provider)` when `DD_AGENT_HOST` is configured and the
/// pipeline builds successfully. As a side effect the OTel global meter
/// provider is set, so callers can subsequently call
/// `opentelemetry::global::meter("harbangan")`.
///
/// The caller must hold the returned provider for the lifetime of the
/// application and pass it to [`shutdown`] to flush pending metric batches.
pub fn init_otel_metrics() -> Option<SdkMeterProvider> {
    let agent_host = std::env::var("DD_AGENT_HOST").ok()?;
    // Validate host: reject values containing URL-special chars that could enable SSRF
    if agent_host.contains('@') || agent_host.contains('/') || agent_host.contains("://") {
        // eprintln! intentional: tracing subscriber is not yet initialized at this call site
        eprintln!("[WARN] DD_AGENT_HOST contains invalid characters — Datadog metrics disabled");
        return None;
    }
    let otlp_port: u16 = std::env::var("DD_OTLP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .filter(|&p: &u16| p > 0)
        .unwrap_or(4318);
    let endpoint = format!("http://{agent_host}:{otlp_port}/v1/metrics");

    match build_metrics_pipeline(&endpoint) {
        Ok(provider) => {
            opentelemetry::global::set_meter_provider(provider.clone());
            // eprintln! intentional: tracing subscriber is not yet initialized at this call site
            eprintln!("[INFO] Datadog OTLP metrics enabled: endpoint={endpoint}");
            Some(provider)
        }
        Err(e) => {
            // eprintln! intentional: tracing subscriber is not yet initialized at this call site
            eprintln!("[WARN] Datadog OTLP metrics init failed ({e}) — metrics disabled");
            None
        }
    }
}

fn build_metrics_pipeline(endpoint: &str) -> anyhow::Result<SdkMeterProvider> {
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::metrics::PeriodicReader;
    use std::time::Duration;

    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .build()
        .context("Failed to create OTLP metrics exporter")?;

    let reader = PeriodicReader::builder(exporter)
        .with_interval(Duration::from_secs(60))
        .build();

    let provider = SdkMeterProvider::builder().with_reader(reader).build();

    Ok(provider)
}

// ── Shutdown ──────────────────────────────────────────────────────────────────

/// Flush buffered spans/metrics and shut down both OTel pipelines.
///
/// Call this once after the HTTP server has fully stopped.
/// Pass the `SdkMeterProvider` returned by [`init_otel_metrics`] if metrics
/// were enabled; passing `None` is a no-op for the metrics side.
pub fn shutdown(metrics_provider: Option<&SdkMeterProvider>) {
    if let Some(provider) = metrics_provider {
        if let Err(e) = provider.shutdown() {
            // eprintln! intentional: tracing subscriber is not yet initialized at this call site
            eprintln!("[WARN] Datadog metrics shutdown error: {e}");
        }
    }
    // Tracer provider shutdown is handled by Drop on the global provider
    // (set via set_tracer_provider in build_trace_pipeline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_datadog_returns_none_when_no_env() {
        temp_env::with_var_unset("DD_AGENT_HOST", || {
            let layer: Option<
                OpenTelemetryLayer<
                    tracing_subscriber::Registry,
                    opentelemetry_sdk::trace::SdkTracer,
                >,
            > = init_datadog();
            assert!(layer.is_none());
        });
    }

    #[test]
    fn test_init_otel_metrics_returns_none_when_no_env() {
        temp_env::with_var_unset("DD_AGENT_HOST", || {
            let provider = init_otel_metrics();
            assert!(provider.is_none());
        });
    }

    #[test]
    fn test_init_datadog_returns_none_on_invalid_host() {
        temp_env::with_var("DD_AGENT_HOST", Some("http://evil@host/path"), || {
            let layer: Option<
                OpenTelemetryLayer<
                    tracing_subscriber::Registry,
                    opentelemetry_sdk::trace::SdkTracer,
                >,
            > = init_datadog();
            assert!(layer.is_none());
        });
    }

    #[test]
    fn test_init_otel_metrics_returns_none_on_invalid_host() {
        temp_env::with_var("DD_AGENT_HOST", Some("http://evil@host/path"), || {
            let provider = init_otel_metrics();
            assert!(provider.is_none());
        });
    }

    #[test]
    fn test_shutdown_no_metrics_is_noop() {
        // Calling shutdown with None should not panic
        shutdown(None);
    }

    #[test]
    fn test_shutdown_with_metrics_provider() {
        use opentelemetry_sdk::metrics::SdkMeterProvider;
        let provider = SdkMeterProvider::builder().build();
        // Should complete without error
        shutdown(Some(&provider));
    }

    // ── Trace ID conversion tests ────────────────────────────────────────

    #[test]
    fn test_otel_trace_id_to_dd_extracts_lower_64_bits() {
        // 128-bit trace ID: upper 64 = 1, lower 64 = 42
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&1u64.to_be_bytes()); // upper 64
        bytes[8..16].copy_from_slice(&42u64.to_be_bytes()); // lower 64
        let trace_id = opentelemetry::trace::TraceId::from_bytes(bytes);

        assert_eq!(otel_trace_id_to_dd(trace_id), "42");
    }

    #[test]
    fn test_otel_trace_id_to_dd_zero() {
        let trace_id = opentelemetry::trace::TraceId::from_bytes([0u8; 16]);
        assert_eq!(otel_trace_id_to_dd(trace_id), "0");
    }

    #[test]
    fn test_otel_trace_id_to_dd_max_lower_64() {
        let mut bytes = [0u8; 16];
        bytes[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
        let trace_id = opentelemetry::trace::TraceId::from_bytes(bytes);

        assert_eq!(otel_trace_id_to_dd(trace_id), u64::MAX.to_string());
    }

    #[test]
    fn test_otel_trace_id_to_dd_known_value() {
        // A realistic trace ID: 463ac35c9f6413ad48485a3953bb6124
        let bytes: [u8; 16] = [
            0x46, 0x3a, 0xc3, 0x5c, 0x9f, 0x64, 0x13, 0xad, 0x48, 0x48, 0x5a, 0x39, 0x53, 0xbb,
            0x61, 0x24,
        ];
        let trace_id = opentelemetry::trace::TraceId::from_bytes(bytes);
        // Lower 64 bits: 0x4848_5a39_53bb_6124
        let expected = u64::from_be_bytes([0x48, 0x48, 0x5a, 0x39, 0x53, 0xbb, 0x61, 0x24]);
        assert_eq!(otel_trace_id_to_dd(trace_id), expected.to_string());
    }

    #[test]
    fn test_otel_span_id_to_dd_decimal() {
        let bytes = 12345u64.to_be_bytes();
        let span_id = opentelemetry::trace::SpanId::from_bytes(bytes);

        assert_eq!(otel_span_id_to_dd(span_id), "12345");
    }

    #[test]
    fn test_otel_span_id_to_dd_zero() {
        let span_id = opentelemetry::trace::SpanId::from_bytes([0u8; 8]);
        assert_eq!(otel_span_id_to_dd(span_id), "0");
    }

    #[test]
    fn test_otel_span_id_to_dd_max() {
        let span_id = opentelemetry::trace::SpanId::from_bytes(u64::MAX.to_be_bytes());
        assert_eq!(otel_span_id_to_dd(span_id), u64::MAX.to_string());
    }

    #[test]
    fn test_otel_span_id_to_dd_known_value() {
        // Span ID: 0x0102_0304_0506_0708
        let bytes: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let span_id = opentelemetry::trace::SpanId::from_bytes(bytes);
        let expected = u64::from_be_bytes(bytes);
        assert_eq!(otel_span_id_to_dd(span_id), expected.to_string());
    }
}
