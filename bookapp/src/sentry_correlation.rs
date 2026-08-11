//! Sentry-OpenTelemetry correlation layer for cross-platform trace correlation.
//!
//! This module provides a custom tracing subscriber layer that enables seamless correlation
//! between Sentry error tracking and OpenTelemetry distributed tracing by automatically
//! adding OpenTelemetry trace and span IDs as tags to Sentry events.
//!
//! # Overview
//!
//! When errors occur in a distributed system, it's crucial to be able to correlate error
//! events in Sentry with the corresponding distributed traces in OpenTelemetry-compatible
//! systems (like Grafana/Tempo). This layer bridges that gap by:
//!
//! 1. Intercepting WARN and ERROR-level tracing events
//! 2. Extracting OpenTelemetry trace context from the current span
//! 3. Adding `otel.trace_id` and `otel.span_id` tags to the Sentry scope
//!
//! # Usage
//!
//! Add the correlation layer to your tracing subscriber stack:
//!
//! ```rust,no_run
//! use tracing_subscriber::layer::SubscriberExt;
//! use sentry_correlation::SentryOtelCorrelationLayer;
//!
//! let subscriber = tracing_subscriber::Registry::default()
//!     .with(opentelemetry_tracing_layer)      // OpenTelemetry layer first
//!     .with(SentryOtelCorrelationLayer::new()) // Correlation bridge
//!     .with(sentry_tracing_layer);            // Sentry layer captures events
//! ```
//!
//! # Layer Ordering
//!
//! **IMPORTANT**: Layer ordering matters for proper correlation:
//!
//! 1. **OpenTelemetry layer** - Creates and manages trace context
//! 2. **SentryOtelCorrelationLayer** - Extracts OTel context for Sentry
//! 3. **Sentry layer** - Captures events with embedded correlation tags
//!
//! # Cross-Platform Debugging Workflow
//!
//! With this correlation in place, incident investigation becomes seamless:
//!
//! 1. 🚨 **Error Alert** - Receive Sentry error notification
//! 2. 🏷️ **Extract Trace ID** - Copy `otel.trace_id` tag from Sentry event
//! 3. 🔍 **Search Traces** - Query Grafana/Tempo for the trace ID
//! 4. 📊 **Analyze Context** - View complete distributed trace context
//! 5. 🎯 **Root Cause** - Identify issue with full request flow visibility

use tracing_subscriber::Layer;

/// A tracing subscriber layer that correlates OpenTelemetry trace context with Sentry events.
///
/// This layer automatically adds OpenTelemetry trace and span IDs as tags to Sentry events,
/// enabling cross-platform correlation between error tracking and distributed tracing systems.
///
/// # Implementation Details
///
/// The layer implements the `tracing_subscriber::Layer` trait and processes events by:
///
/// 1. Filtering for WARNING and ERROR-level events
/// 2. Extracting OpenTelemetry context from the event's span
/// 3. Adding correlation tags to the Sentry scope
///
/// # Performance Considerations
///
/// - Minimal overhead: only processes WARNING and ERROR-level events by default
/// - Non-blocking: correlation happens synchronously but quickly
/// - Graceful degradation: continues working even if OTel context is unavailable
///
/// # Example
///
/// ```rust,no_run
/// use sentry_correlation::SentryOtelCorrelationLayer;
/// use tracing_subscriber::layer::SubscriberExt;
///
/// // Add to subscriber stack
/// let subscriber = tracing_subscriber::Registry::default()
///     .with(SentryOtelCorrelationLayer::new());
///
/// // Later, when an error occurs:
/// tracing::error!("Database connection failed");
/// // Sentry event will automatically include otel.trace_id and otel.span_id tags
/// ```
pub struct SentryOtelCorrelationLayer {
    /// The minimum tracing level that triggers correlation.
    /// Defaults to WARN, which includes WARN and ERROR events.
    min_level: tracing::Level,
}

impl SentryOtelCorrelationLayer {
    /// Creates a new correlation layer with default settings.
    ///
    /// By default, WARNING and ERROR-level events trigger correlation.
    pub fn new() -> Self {
        Self {
            min_level: tracing::Level::WARN,
        }
    }

    fn should_correlate(&self, event_level: &tracing::Level) -> bool {
        // `tracing::Level` is ordered from most to least severe:
        // ERROR < WARN < INFO < DEBUG < TRACE.
        event_level <= &self.min_level
    }

    /// Extracts OpenTelemetry trace context and adds it to Sentry scope.
    ///
    /// This method attempts to extract trace and span IDs from the OpenTelemetry
    /// context associated with the tracing span and adds them as tags to the
    /// current Sentry scope.
    ///
    /// # Tags Added
    ///
    /// - `otel.trace_id`: OpenTelemetry trace ID (32-character hex string)
    /// - `otel.span_id`: OpenTelemetry span ID (16-character hex string)
    fn correlate_with_sentry<S>(
        &self,
        ctx: &tracing_subscriber::layer::Context<'_, S>,
        event: &tracing::Event<'_>,
    ) where
        S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    {
        let otel_ids = ctx.event_span(event).and_then(|span_ref| {
            let extensions = span_ref.extensions();
            let otel_data = extensions.get::<tracing_opentelemetry::OtelData>()?;
            Some((otel_data.trace_id()?, otel_data.span_id()?))
        });

        sentry::configure_scope(|scope| match otel_ids {
            Some((trace_id, span_id)) => {
                scope.set_tag("otel.trace_id", format!("{trace_id:032x}"));
                scope.set_tag("otel.span_id", format!("{span_id:016x}"));
            }
            None => {
                scope.remove_tag("otel.trace_id");
                scope.remove_tag("otel.span_id");
            }
        });
    }
}

impl Default for SentryOtelCorrelationLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for SentryOtelCorrelationLayer
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    /// Processes tracing events and adds OpenTelemetry correlation to Sentry.
    ///
    /// This method is called for every tracing event. It filters events based on
    /// the configured minimum level and attempts to correlate OpenTelemetry trace
    /// context with Sentry for qualifying events.
    ///
    /// # Performance
    ///
    /// - Fast path: Non-qualifying events are filtered out immediately
    /// - Graceful degradation: Missing OTel context doesn't cause errors
    /// - Minimal allocations: Only formats trace IDs when correlation succeeds
    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // Only process events at or more severe than the configured level.
        if self.should_correlate(event.metadata().level()) {
            self.correlate_with_sentry(&ctx, event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{TraceContextExt, TracerProvider};
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing::Level;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn test_new_layer_defaults_to_warn_level() {
        let layer = SentryOtelCorrelationLayer::new();
        assert_eq!(layer.min_level, Level::WARN);
    }

    #[test]
    fn test_default_implementation() {
        let layer = SentryOtelCorrelationLayer::default();
        assert_eq!(layer.min_level, Level::WARN);
    }

    #[test]
    fn warn_threshold_includes_error_and_warn_only() {
        let layer = SentryOtelCorrelationLayer::new();

        assert!(layer.should_correlate(&Level::ERROR));
        assert!(layer.should_correlate(&Level::WARN));
        assert!(!layer.should_correlate(&Level::INFO));
        assert!(!layer.should_correlate(&Level::DEBUG));
        assert!(!layer.should_correlate(&Level::TRACE));
    }

    #[test]
    fn event_without_an_active_span_clears_existing_correlation_tags() {
        let subscriber = tracing_subscriber::registry().with(SentryOtelCorrelationLayer::new());
        let events = sentry::test::with_captured_events(|| {
            sentry::configure_scope(|scope| {
                scope.set_tag("otel.trace_id", "stale-trace");
                scope.set_tag("otel.span_id", "stale-span");
            });

            tracing::subscriber::with_default(subscriber, || {
                tracing::warn!("event without an active span");
            });
            sentry::capture_message("captured without span context", sentry::Level::Warning);
        });

        let event = events.first().expect("one captured Sentry event");
        assert_eq!(events.len(), 1);
        assert!(!event.tags.contains_key("otel.trace_id"));
        assert!(!event.tags.contains_key("otel.span_id"));
    }

    #[test]
    fn event_without_otel_data_clears_existing_correlation_tags() {
        let subscriber = tracing_subscriber::registry().with(SentryOtelCorrelationLayer::new());
        let events = sentry::test::with_captured_events(|| {
            sentry::configure_scope(|scope| {
                scope.set_tag("otel.trace_id", "stale-trace");
                scope.set_tag("otel.span_id", "stale-span");
            });

            tracing::subscriber::with_default(subscriber, || {
                let span = tracing::info_span!("span-without-otel-data");
                let _guard = span.enter();
                tracing::error!("event without OTel data");
            });
            sentry::capture_message("captured after cleanup", sentry::Level::Error);
        });

        let event = events.first().expect("one captured Sentry event");
        assert_eq!(events.len(), 1);
        assert!(!event.tags.contains_key("otel.trace_id"));
        assert!(!event.tags.contains_key("otel.span_id"));
    }

    #[test]
    fn error_event_uses_the_current_child_span_context() {
        let provider = SdkTracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("correlation-test")))
            .with(SentryOtelCorrelationLayer::new());

        let mut expected_trace_id = None;
        let mut expected_span_id = None;
        let events = sentry::test::with_captured_events(|| {
            tracing::subscriber::with_default(subscriber, || {
                let parent = tracing::info_span!("parent");
                let _parent_guard = parent.enter();
                let parent_context = parent.context();

                let child = tracing::info_span!("child");
                let _child_guard = child.enter();
                let child_context = child.context();

                let parent_span = parent_context.span();
                let parent_span_context = parent_span.span_context();
                let child_span = child_context.span();
                let child_span_context = child_span.span_context();

                assert_eq!(
                    parent_span_context.trace_id(),
                    child_span_context.trace_id(),
                    "a child and its parent belong to the same trace"
                );
                assert_ne!(
                    parent_span_context.span_id(),
                    child_span_context.span_id(),
                    "each span has its own span ID"
                );

                tracing::error!("capture current child context");
                sentry::capture_message("captured after error", sentry::Level::Error);

                expected_trace_id = Some(child_span_context.trace_id().to_string());
                expected_span_id = Some(child_span_context.span_id().to_string());

                sentry::configure_scope(|scope| {
                    scope.remove_tag("otel.trace_id");
                    scope.remove_tag("otel.span_id");
                });
            });
        });

        let event = events.first().expect("one captured Sentry event");
        assert_eq!(events.len(), 1);
        assert_eq!(event.tags.get("otel.trace_id"), expected_trace_id.as_ref());
        assert_eq!(event.tags.get("otel.span_id"), expected_span_id.as_ref());
    }

    #[test]
    fn scope_tags_are_attached_to_later_events_until_removed() {
        let events = sentry::test::with_captured_events(|| {
            sentry::configure_scope(|scope| {
                scope.set_tag("otel.trace_id", "first-operation-trace");
            });

            sentry::capture_message("first event", sentry::Level::Error);
            sentry::capture_message("later event", sentry::Level::Error);

            sentry::configure_scope(|scope| {
                scope.remove_tag("otel.trace_id");
            });
            sentry::capture_message("event after cleanup", sentry::Level::Error);
        });

        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].tags.get("otel.trace_id").map(String::as_str),
            Some("first-operation-trace")
        );
        assert_eq!(
            events[1].tags.get("otel.trace_id").map(String::as_str),
            Some("first-operation-trace")
        );
        assert!(!events[2].tags.contains_key("otel.trace_id"));
    }
}
