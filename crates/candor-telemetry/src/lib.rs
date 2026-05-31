//! OpenTelemetry OTLP exporter wiring for Candor AI node transition tracing.
//!
//! Initializes a tracing subscriber with an optional OTLP gRPC exporter.
//! Falls back to a plain `fmt` subscriber when no endpoint is configured
//! or the exporter fails to connect — the agent always logs something.
//!
//! # Usage
//!
//! ```ignore
//! let _guard = candor_telemetry::init_telemetry(
//!     "candor-daemon",
//!     Some("http://localhost:4317"),
//! );
//!
//! // Instrument node transitions with tracing spans:
//! let span = tracing::info_span!("node.execute", node_id = %node_id, phase = "think");
//! let _guard = span.enter();
//! ```
//!
//! See also [`tracing::info_span!`] / [`tracing::Span`] for the full API
//! available to instrument node execution, tool calls, phase transitions,
//! and API call timing.

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

// ── Public API ───────────────────────────────────────────────────────────────

/// Initialise the tracing subscriber and optionally an OTLP gRPC exporter.
///
/// `service_name`    – value for the `service.name` resource attribute.
/// `otlp_endpoint`   – full gRPC URL (e.g. `http://localhost:4317`).
///                     Pass `None` or `Some("")` to skip OTLP and use
///                     a plain `fmt` subscriber instead.
///
/// Returns a [`TelemetryGuard`] that **must** be held alive for the entire
/// lifetime of the program.  Dropping the guard flushes all pending spans
/// and shuts down the OTLP exporter gracefully.
#[must_use]
pub fn init_telemetry(service_name: &str, otlp_endpoint: Option<&str>) -> TelemetryGuard {
    let endpoint = otlp_endpoint.and_then(|e| {
        if e.is_empty() {
            None
        } else {
            Some(e.to_owned())
        }
    });

    let provider = if let Some(ep) = endpoint {
        match build_otlp_provider(service_name, &ep) {
            Ok(provider) => {
                let tracer = provider.tracer(service_name.to_owned());
                let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

                tracing_subscriber::registry()
                    .with(otel_layer)
                    .with(tracing_subscriber::EnvFilter::from_default_env())
                    .with(
                        tracing_subscriber::fmt::layer()
                            .with_target(true)
                            .with_thread_ids(true),
                    )
                    .init();

                Some(provider)
            }
            Err(e) => {
                eprintln!(
                    "[candor-telemetry] Failed to build OTLP exporter at {ep}: \
                     {e}.  Falling back to fmt subscriber."
                );
                init_fmt_subscriber();
                None
            }
        }
    } else {
        init_fmt_subscriber();
        None
    };

    TelemetryGuard { provider }
}

/// Force-flush and shut down the tracer provider (if any).
///
/// Normally you don't need to call this — [`TelemetryGuard`] does it on drop.
/// Call it explicitly when you need synchronous flushing before a known
/// termination point (e.g. signal handler).
pub fn shutdown_telemetry(guard: TelemetryGuard) {
    drop(guard);
}

// ── Guard ────────────────────────────────────────────────────────────────────

/// RAII guard whose drop triggers OTLP flush + shutdown.
///
/// Created by [`init_telemetry`].  **Must outlive** every span that should be
/// exported.  Typically bound with `let _guard = …` at the top of `main()`.
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            tracing_opentelemetry::OpenTelemetrySpanExt::set_parent(
                &tracing::Span::current(),
                opentelemetry::Context::new(),
            );
            if let Err(e) = provider.shutdown() {
                eprintln!("[candor-telemetry] OTLP shutdown error: {e}");
            }
        }
    }
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn build_otlp_provider(
    service_name: &str,
    endpoint: &str,
) -> Result<SdkTracerProvider, opentelemetry_otlp::ExporterBuildError> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    let resource = Resource::builder_empty()
        .with_attribute(KeyValue::new("service.name", service_name.to_owned()))
        .build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    Ok(provider)
}

fn init_fmt_subscriber() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with_target(true)
        .with_thread_ids(true)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_noop_fmt() {
        // When no endpoint is given the subscriber should still init without panicking
        let _guard = init_telemetry("test-service", None);
        tracing::info!("no-op fmt subscriber works");
    }
}
