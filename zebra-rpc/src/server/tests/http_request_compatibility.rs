//! Tests for the HTTP request compatibility middleware.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http_body::Frame;
use http_body_util::BodyExt;
use hyper::header;
use jsonrpsee::{
    core::BoxError,
    server::{HttpBody, HttpRequest, HttpResponse},
};
use tower::Service;

use crate::server::http_request_compatibility::HttpRequestMiddleware;

/// A body that always returns an error, simulating a TCP RST during body collection.
struct ErrorBody;

impl http_body::Body for ErrorBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(Some(Err("connection reset".into())))
    }
}

/// A body that never returns a frame, simulating a client that keeps the
/// request body open.
struct PendingBody;

impl http_body::Body for PendingBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Pending
    }
}

/// A mock inner service that returns a minimal JSON-RPC 2.0 response.
#[derive(Clone)]
struct MockRpcService;

impl Service<HttpRequest> for MockRpcService {
    type Response = HttpResponse;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, BoxError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: HttpRequest) -> Self::Future {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let response = HttpResponse::new(HttpBody::from(body.to_string()));
        Box::pin(async { Ok(response) })
    }
}

/// A mock inner service that records when it is called.
#[derive(Clone)]
struct RecordingRpcService {
    was_called: Arc<AtomicBool>,
}

impl Service<HttpRequest> for RecordingRpcService {
    type Response = HttpResponse;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, BoxError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: HttpRequest) -> Self::Future {
        self.was_called.store(true, Ordering::SeqCst);

        let body = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let response = HttpResponse::new(HttpBody::from(body.to_string()));
        Box::pin(async { Ok(response) })
    }
}

/// A mock inner service that asserts the request content type it receives.
#[derive(Clone)]
struct ContentTypeAssertingRpcService {
    expected_content_type: &'static str,
}

impl Service<HttpRequest> for ContentTypeAssertingRpcService {
    type Response = HttpResponse;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, BoxError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let actual_content_type = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());

        assert_eq!(actual_content_type, Some(self.expected_content_type));

        let body = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let response = HttpResponse::new(HttpBody::from(body.to_string()));
        Box::pin(async { Ok(response) })
    }
}

/// A mock inner service that records the raw request body it receives.
#[derive(Clone)]
struct BodyRecordingRpcService {
    captured_body: Arc<Mutex<Option<Vec<u8>>>>,
    response_body: &'static str,
}

impl Service<HttpRequest> for BodyRecordingRpcService {
    type Response = HttpResponse;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, BoxError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let captured_body = self.captured_body.clone();
        let response_body = self.response_body;

        Box::pin(async move {
            let body = req.into_body().collect().await?.to_bytes().to_vec();
            *captured_body
                .lock()
                .expect("test capture mutex should not be poisoned") = Some(body);

            Ok(HttpResponse::new(HttpBody::from(response_body.to_string())))
        })
    }
}

/// Verifies that body collection errors return `Err` instead of panicking.
///
/// Previously, the middleware called `.expect()` on `body.collect().await`,
/// so a TCP RST during body reading would panic the process.
#[tokio::test]
async fn request_body_error_returns_err_instead_of_panic() {
    let error_body = HttpBody::new(ErrorBody);
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "appliion/json")
        .body(error_body)
        .expect("valid request");

    let mut middleware = HttpRequestMiddleware::new(MockRpcService, None, 2_097_152);
    let result = middleware.call(request).await;

    assert!(
        result.is_err(),
        "body collection error should return Err, not panic"
    );
}

/// Verifies that a request body exceeding `max_request_body_size` is rejected.
#[tokio::test]
async fn oversized_request_body_is_rejected() {
    let limit = 64;
    let oversized = vec![b'x'; limit + 1];
    let body = HttpBody::from(oversized);
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(body)
        .expect("valid request");

    let mut middleware = HttpRequestMiddleware::new(MockRpcService, None, limit);
    let result = middleware.call(request).await;

    assert!(result.is_err(), "oversized request body should be rejected");
}

/// Verifies that an auth-disabled request with an incomplete body stays in
/// Zebra's compatibility middleware before reaching the inner RPC service.
#[tokio::test]
async fn pending_body_waits_before_inner_service_today() {
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(HttpBody::new(PendingBody))
        .expect("valid request");

    let was_called = Arc::new(AtomicBool::new(false));
    let mut middleware = HttpRequestMiddleware::new(
        RecordingRpcService {
            was_called: was_called.clone(),
        },
        None,
        2_097_152,
    );

    let result = tokio::time::timeout(Duration::from_millis(10), middleware.call(request)).await;

    assert!(
        result.is_err(),
        "pending body collection should keep the middleware future pending today"
    );
    assert!(
        !was_called.load(Ordering::SeqCst),
        "the inner RPC service should not be called before body collection finishes"
    );
}

/// Verifies that `text/plain` requests are rewritten to `application/json`
/// before reaching the inner RPC service when auth is disabled.
#[tokio::test]
async fn text_plain_request_is_rewritten_to_json_without_auth_today() {
    let body =
        HttpBody::from(r#"{"jsonrpc":"2.0","id":1,"method":"getinfo","params":[]}"#.to_string());
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "text/plain; charset=utf-8")
        .body(body)
        .expect("valid request");

    let mut middleware = HttpRequestMiddleware::new(
        ContentTypeAssertingRpcService {
            expected_content_type: "application/json",
        },
        None,
        2_097_152,
    );
    let result = middleware.call(request).await;

    assert!(
        result.is_ok(),
        "auth-disabled text/plain request should reach the inner service today"
    );
}

/// Verifies that strict JSON-RPC 2.0 request bodies are parsed and reserialized
/// by Zebra's compatibility middleware before reaching the inner RPC service.
#[tokio::test]
async fn strict_json_rpc_2_request_is_reserialized_before_inner_service_today() {
    let original_body =
        r#"{ "params" : [1, 2], "method" : "getinfo", "id" : 1, "jsonrpc" : "2.0" }"#;
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(HttpBody::from(original_body.to_string()))
        .expect("valid request");

    let captured_body = Arc::new(Mutex::new(None));
    let mut middleware = HttpRequestMiddleware::new(
        BodyRecordingRpcService {
            captured_body: captured_body.clone(),
            response_body: r#"{"jsonrpc":"2.0","id":1,"result":null}"#,
        },
        None,
        2_097_152,
    );

    let result = middleware.call(request).await;

    assert!(
        result.is_ok(),
        "strict JSON-RPC 2.0 request should reach the inner service"
    );

    let captured_body = captured_body
        .lock()
        .expect("test capture mutex should not be poisoned")
        .clone()
        .expect("inner service should capture the request body");
    let captured_body = String::from_utf8(captured_body).expect("captured body should be UTF-8");

    assert_ne!(
        captured_body, original_body,
        "strict JSON-RPC 2.0 requests are reserialized before inner dispatch today"
    );
    assert_eq!(
        captured_body,
        r#"{"jsonrpc":"2.0","method":"getinfo","params":[1,2],"id":1}"#
    );
}

/// Verifies that strict JSON-RPC 2.0 response bodies are parsed and reserialized
/// by Zebra's compatibility middleware before they are returned to the caller.
#[tokio::test]
async fn strict_json_rpc_2_response_is_reserialized_before_client_today() {
    let request = HttpRequest::builder()
        .method("POST")
        .header("content-type", "application/json")
        .body(HttpBody::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"getinfo","params":[]}"#.to_string(),
        ))
        .expect("valid request");

    let captured_body = Arc::new(Mutex::new(None));
    let original_response_body = r#"{ "id" : 1, "result" : null, "jsonrpc" : "2.0" }"#;
    let mut middleware = HttpRequestMiddleware::new(
        BodyRecordingRpcService {
            captured_body,
            response_body: original_response_body,
        },
        None,
        2_097_152,
    );

    let response = middleware
        .call(request)
        .await
        .expect("strict JSON-RPC 2.0 request should succeed");
    let response_body = response
        .into_body()
        .collect()
        .await
        .expect("response body should collect")
        .to_bytes();
    let response_body =
        String::from_utf8(response_body.to_vec()).expect("response should be UTF-8");

    assert_ne!(
        response_body, original_response_body,
        "strict JSON-RPC 2.0 responses are reserialized before returning today"
    );
    assert_eq!(response_body, r#"{"jsonrpc":"2.0","id":1,"result":null}"#);
}
