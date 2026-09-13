//! Per-browser [`cef::RequestContext`] wiring (see `examples/osr`).

use cef::*;

#[derive(Clone)]
pub struct VmuxRequestContextHandler {}

wrap_request_context_handler! {
    pub struct VmuxRequestContextHandlerBuilder {
        handler: VmuxRequestContextHandler,
    }

    impl RequestContextHandler {}
}

impl VmuxRequestContextHandlerBuilder {
    pub fn build(handler: VmuxRequestContextHandler) -> RequestContextHandler {
        Self::new(handler)
    }
}
