use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Request;
use std::str::FromStr;
use tracing::Span;

/// Injects the given OpenTelemetry Context into a reqwest::Request headers to allow propagation downstream.
pub fn inject_opentelemetry_context_into_request(request: &mut Request) {
    opentelemetry::global::get_text_map_propagator(|injector| {
        use tracing_opentelemetry::OpenTelemetrySpanExt;
        let context = Span::current().context();
        injector.inject_context(&context, &mut RequestCarrier::new(request))
    });
}

// "traceparent" => https://www.w3.org/TR/trace-context/#trace-context-http-headers-format

/// Injector used via opentelemetry propagator to tell the extractor how to insert the "traceparent" header value
/// This will allow the propagator to inject opentelemetry context into a standard data structure. Will basically
/// insert a "traceparent" string value "{version}-{trace_id}-{span_id}-{trace-flags}" of the spans context into the headers.
/// Listeners can then re-hydrate the context to add additional spans to the same trace.
struct RequestCarrier<'a> {
    request: &'a mut Request,
}

impl<'a> RequestCarrier<'a> {
    pub fn new(request: &'a mut Request) -> Self {
        RequestCarrier { request }
    }

    fn set_inner(&mut self, key: &str, value: String) {
        let header_name = HeaderName::from_str(key).expect("Must be header name");
        let header_value = HeaderValue::from_str(&value).expect("Must be a header value");
        self.request.headers_mut().insert(header_name, header_value);
    }
}

impl opentelemetry::propagation::Injector for RequestCarrier<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.set_inner(key, value)
    }
}
