//! Tests for JSON-RPC batch request handling.

use std::{
    future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use http_body_util::BodyExt;
use jsonrpsee::{
    server::{
        http::call_with_service, middleware::rpc::RpcServiceT, BatchRequestConfig, HttpBody,
        HttpRequest, MethodResponse,
    },
    types::Request,
    ResponsePayload,
};

#[derive(Clone, Debug)]
struct CountingRpcService {
    calls: Arc<AtomicUsize>,
}

impl<'a> RpcServiceT<'a> for CountingRpcService {
    type Future = future::Ready<MethodResponse>;

    fn call(&self, request: Request<'a>) -> Self::Future {
        self.calls.fetch_add(1, Ordering::SeqCst);

        future::ready(MethodResponse::response(
            request.id().into_owned(),
            ResponsePayload::success("ok"),
            usize::MAX,
        ))
    }
}

/// Verifies the jsonrpsee behavior Zebra inherits by not configuring
/// `BatchRequestConfig`: a single HTTP request can dispatch every batch entry.
#[tokio::test]
async fn unlimited_batch_request_dispatches_every_call_today() {
    let body = r#"[
        {"jsonrpc":"2.0","id":1,"method":"a"},
        {"jsonrpc":"2.0","id":2,"method":"b"},
        {"jsonrpc":"2.0","id":3,"method":"c"}
    ]"#;
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(HttpBody::from(body.to_string()))
        .expect("test request should be valid");

    let calls = Arc::new(AtomicUsize::new(0));
    let response = call_with_service(
        request,
        BatchRequestConfig::Unlimited,
        body.len() as u32,
        CountingRpcService {
            calls: calls.clone(),
        },
        1024 * 1024,
    )
    .await;

    let response_body = response
        .into_body()
        .collect()
        .await
        .expect("response body should collect")
        .to_bytes();
    let response_body =
        std::str::from_utf8(&response_body).expect("JSON-RPC response should be UTF-8");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "Unlimited batch handling dispatches each call in the batch today"
    );
    assert!(
        response_body.contains(r#""id":1"#)
            && response_body.contains(r#""id":2"#)
            && response_body.contains(r#""id":3"#),
        "batch response should include every dispatched call: {response_body}"
    );
}
