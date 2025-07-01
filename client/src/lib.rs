mod generated;
mod otel;

pub use otel::inject_opentelemetry_context_into_request;

pub use generated::*;

/// State maintained by a [`Client`].
/// Currently empty but required to use the with_pre_hook_async functionality.
#[derive(Clone, Debug, Default)]
pub struct ClientState {}
