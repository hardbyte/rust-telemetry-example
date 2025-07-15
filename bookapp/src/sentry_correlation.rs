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
//! 1. Intercepting ERROR-level tracing events
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

use opentelemetry::trace::TraceContextExt;
use tracing_opentelemetry::OpenTelemetrySpanExt;
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
/// 1. Filtering for WARNING and ERROR-level events (configurable)
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
    /// Defaults to ERROR to minimize performance impact.
    min_level: tracing::Level,
}

impl SentryOtelCorrelationLayer {
    /// Creates a new correlation layer with default settings.
    ///
    /// By default, WARNING and ERROR-level events trigger correlation.
    /// Use `with_level()` to customize this behavior.
    pub fn new() -> Self {
        Self {
            min_level: tracing::Level::WARN,
        }
    }

    /// Creates a correlation layer that processes events at the specified level and above.
    ///
    /// # Arguments
    ///
    /// * `level` - Minimum tracing level that triggers correlation
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use sentry_correlation::SentryOtelCorrelationLayer;
    /// use tracing::Level;
    ///
    /// // Correlate both ERROR and WARN level events
    /// let layer = SentryOtelCorrelationLayer::with_level(Level::WARN);
    /// ```
    pub fn with_level(level: tracing::Level) -> Self {
        Self { min_level: level }
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
        // Try to get OpenTelemetry context from the event's span
        if let Some(span_ref) = ctx.event_span(event) {
            // Get OpenTelemetry extensions from the span
            if let Some(otel_data) = span_ref.extensions().get::<tracing_opentelemetry::OtelData>() {
                let parent_cx = &otel_data.parent_cx;
                let span_ref = parent_cx.span();
                let span_context = span_ref.span_context();

                if span_context.is_valid() {
                    let trace_id = span_context.trace_id();
                    let span_id = span_context.span_id();

                    // Add OpenTelemetry context to Sentry scope for cross-platform correlation
                    sentry::configure_scope(|scope| {
                        scope.set_tag("otel.trace_id", &format!("{:032x}", trace_id));
                        scope.set_tag("otel.span_id", &format!("{:016x}", span_id));
                    });
                }
            }
        }
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
        // Only process events at or above the configured level
        if event.metadata().level() >= &self.min_level {
            self.correlate_with_sentry(&ctx, event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    #[test]
    fn test_new_layer_defaults_to_warn_level() {
        let layer = SentryOtelCorrelationLayer::new();
        assert_eq!(layer.min_level, Level::WARN);
    }

    #[test]
    fn test_with_level_sets_custom_level() {
        let layer = SentryOtelCorrelationLayer::with_level(Level::WARN);
        assert_eq!(layer.min_level, Level::WARN);
    }

    #[test]
    fn test_default_implementation() {
        let layer = SentryOtelCorrelationLayer::default();
        assert_eq!(layer.min_level, Level::WARN);
    }
}