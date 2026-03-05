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
//! # Metrics
//!
//! Call [`init_otel_metrics`] to initialise a `SdkMeterProvider` that exports
//! metrics via OTLP-HTTP to the Datadog Agent.  When `Some` is returned, the
//! global OTel meter provider is already set — use
//! `opentelemetry::global::meter("kiro-gateway")` to obtain a `Meter`.
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
//! | `DD_SERVICE`      | `kiro-gateway` | APM service name                         |
//! | `DD_ENV`          | *unset*        | Deployment environment tag               |
//! | `DD_VERSION`      | *unset*        | Service version tag                      |

use anyhow::Context as _;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;

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
pub fn init_datadog<S>() -> Option<OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
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
    let service_name = std::env::var("DD_SERVICE").unwrap_or_else(|_| "kiro-gateway".to_string());
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
            eprintln!("[WARN] Datadog APM init failed ({e}) — tracing disabled");
            None
        }
    }
}

fn build_trace_pipeline<S>(
    service_name: &str,
    agent_endpoint: &str,
) -> anyhow::Result<OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let tracer = opentelemetry_datadog::new_pipeline()
        .with_service_name(service_name)
        .with_agent_endpoint(agent_endpoint)
        .with_api_version(opentelemetry_datadog::ApiVersion::Version05)
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .context("Failed to install Datadog tracing pipeline")?;

    Ok(tracing_opentelemetry::layer().with_tracer(tracer))
}

// ── Metrics ───────────────────────────────────────────────────────────────────

/// Initialise the OTLP metrics pipeline targeting the Datadog Agent.
///
/// Returns `Some(provider)` when `DD_AGENT_HOST` is configured and the
/// pipeline builds successfully. As a side effect the OTel global meter
/// provider is set, so callers can subsequently call
/// `opentelemetry::global::meter("kiro-gateway")`.
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

    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
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
    opentelemetry::global::shutdown_tracer_provider();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_datadog_returns_none_when_no_env() {
        temp_env::with_var_unset("DD_AGENT_HOST", || {
            let layer: Option<OpenTelemetryLayer<tracing_subscriber::Registry, _>> = init_datadog();
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
            let layer: Option<OpenTelemetryLayer<tracing_subscriber::Registry, _>> = init_datadog();
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
}
