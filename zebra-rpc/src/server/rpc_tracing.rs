//! RPC tracing middleware for OpenTelemetry SERVER spans.
//!
//! This middleware enables Jaeger Service Performance Monitoring (SPM) by marking
//! JSON-RPC endpoints with `SPAN_KIND_SERVER`. SPM displays RED metrics (Rate, Errors,
//! Duration) for server-side handlers.
//!
//! # Background
//!
//! Jaeger SPM filters for `SPAN_KIND_SERVER` spans by default. Without this middleware,
//! all Zebra spans are `SPAN_KIND_INTERNAL`, making them invisible in SPM's Monitor tab.
//!
//! This follows OpenTelemetry best practices used by other blockchain clients:
//! - Lighthouse (Ethereum consensus client)
//! - Hyperledger Besu (Ethereum execution client)
//! - Hyperledger Fabric

use jsonrpsee::{
    server::middleware::rpc::{layer::ResponseFuture, RpcServiceT},
    MethodResponse,
};
use tracing::{info_span, Instrument};

/// Middleware that creates SERVER spans for each RPC request.
///
/// This enables Jaeger SPM by marking RPC endpoints as server-side handlers.
/// Also captures error codes and messages for debugging.
///
/// # OpenTelemetry Attributes
///
/// Each span includes:
/// - `otel.kind = "server"` - Marks this as a server span for SPM
/// - `rpc.method` - The JSON-RPC method name (e.g., "getinfo", "getblock")
/// - `rpc.system = "jsonrpc"` - The RPC protocol
/// - `otel.status_code` - "ERROR" on failure (empty on success)
/// - `rpc.error_code` - JSON-RPC error code on failure
#[derive(Clone)]
pub struct RpcTracingMiddleware<S> {
    service: S,
}

impl<S> RpcTracingMiddleware<S> {
    /// Create a new `RpcTracingMiddleware` with the given `service`.
    pub fn new(service: S) -> Self {
        Self { service }
    }
}

impl<'a, S> RpcServiceT<'a> for RpcTracingMiddleware<S>
where
    S: RpcServiceT<'a> + Send + Sync + Clone + 'static,
{
    type Future = ResponseFuture<futures::future::BoxFuture<'a, MethodResponse>>;

    fn call(&self, request: jsonrpsee::types::Request<'a>) -> Self::Future {
        let service = self.service.clone();
        let method = request.method_name().to_owned();

        // Create span OUTSIDE the async block so it's properly registered
        // with the tracing subscriber before the future starts.
        let span = info_span!(
            "rpc_request",
            otel.kind = "server",
            rpc.method = %method,
            rpc.system = "jsonrpc",
            otel.status_code = tracing::field::Empty,
            rpc.error_code = tracing::field::Empty,
        );

        // Clone span for recording after response
        let span_for_record = span.clone();

        // Instrument the ENTIRE future with the span
        ResponseFuture::future(Box::pin(
            async move {
                let response = service.call(request).await;

                // Record error details if the response is an error
                if response.is_error() {
                    span_for_record.record("otel.status_code", "ERROR");
                    if let Some(error_code) = response.as_error_code() {
                        span_for_record.record("rpc.error_code", error_code);
                    }
                }

                response
            }
            .instrument(span),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        borrow::Cow,
        future,
        sync::{Arc, Mutex},
    };

    use jsonrpsee::{
        types::{Id, Request},
        ResponsePayload,
    };
    use tracing::field::{Field, Visit};
    use tracing_subscriber::{layer::Context, prelude::*, registry::LookupSpan, Layer};

    #[derive(Clone, Debug)]
    struct SuccessRpcService;

    impl<'a> RpcServiceT<'a> for SuccessRpcService {
        type Future = future::Ready<MethodResponse>;

        fn call(&self, request: Request<'a>) -> Self::Future {
            future::ready(MethodResponse::response(
                request.id().into_owned(),
                ResponsePayload::success("ok"),
                usize::MAX,
            ))
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSpanLayer {
        rpc_methods: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingSpanLayer {
        fn recorded_rpc_methods(&self) -> Vec<String> {
            self.rpc_methods
                .lock()
                .expect("span recorder mutex should not be poisoned because tests do not panic while holding it")
                .clone()
        }
    }

    impl<S> Layer<S> for RecordingSpanLayer
    where
        S: tracing::Subscriber + for<'span> LookupSpan<'span>,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _id: &tracing::Id,
            _ctx: Context<'_, S>,
        ) {
            if attrs.metadata().name() != "rpc_request" {
                return;
            }

            let mut visitor = RpcMethodVisitor::default();
            attrs.record(&mut visitor);

            if let Some(method) = visitor.rpc_method {
                self.rpc_methods
                    .lock()
                    .expect("span recorder mutex should not be poisoned because tests do not panic while holding it")
                    .push(method);
            }
        }
    }

    #[derive(Default)]
    struct RpcMethodVisitor {
        rpc_method: Option<String>,
    }

    impl Visit for RpcMethodVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "rpc.method" {
                self.rpc_method = Some(value.to_string());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "rpc.method" {
                self.rpc_method = Some(format!("{value:?}"));
            }
        }
    }

    #[test]
    fn rpc_tracing_method_attribute_uses_raw_unknown_method_today() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime should build for this tracing capture test");
        let service = RpcTracingMiddleware::new(SuccessRpcService);
        let span_layer = RecordingSpanLayer::default();
        let subscriber = tracing_subscriber::registry().with(span_layer.clone());
        let unknown_methods = [
            "unknown_tracing_probe_method_a",
            "unknown_tracing_probe_method_b",
        ];

        tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(async {
                for (id, method) in unknown_methods.iter().enumerate() {
                    let response = service
                        .call(Request::new(
                            Cow::Borrowed(*method),
                            None,
                            Id::Number(id as u64),
                        ))
                        .await;

                    assert!(
                        !response.is_error(),
                        "test service should return success for method {method}"
                    );
                }
            });
        });

        let rpc_methods = span_layer.recorded_rpc_methods();
        for method in unknown_methods {
            assert!(
                rpc_methods.iter().any(|recorded| recorded == method),
                "tracing span should use the raw unknown method as rpc.method: {rpc_methods:?}"
            );
        }
    }
}
