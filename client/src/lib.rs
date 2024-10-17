mod generated;
mod otel;

pub use otel::inject_opentelemetry_context_into_request;

pub use generated::*;

/// State maintained by a [`Client`].
/// Currently empty but required to use the with_pre_hook_async functionality
/// with progenitor as of our pinned version https://github.com/oxidecomputer/progenitor/blob/4a3dfec3926f1f9db78eb6dc90087a1e2a1f9e45/progenitor-impl/src/method.rs#L1144-L1151
#[derive(Clone, Debug, Default)]
pub struct ClientState {}

