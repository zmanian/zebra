//! RPC metrics middleware for Prometheus metrics collection.
//!
//! This middleware collects metrics for JSON-RPC requests, including:
//! - Request count by method and status
//! - Request duration by method
//! - Active request count
//! - Error count by method and error code
//!
//! These metrics complement the OpenTelemetry tracing in `rpc_tracing.rs`,
//! providing aggregated data suitable for dashboards and alerting.

use std::time::Instant;

use jsonrpsee::{
    server::middleware::rpc::{layer::ResponseFuture, RpcServiceT},
    MethodResponse,
};

/// Middleware that collects Prometheus metrics for each RPC request.
///
/// This middleware records:
/// - `rpc.requests.total{method, status}` - Counter of requests by method and status
/// - `rpc.request.duration_seconds{method}` - Histogram of request durations
/// - `rpc.active_requests` - Gauge of currently active requests
/// - `rpc.errors.total{method, error_code}` - Counter of errors by method and code
#[derive(Clone)]
pub struct RpcMetricsMiddleware<S> {
    service: S,
}

impl<S> RpcMetricsMiddleware<S> {
    /// Create a new `RpcMetricsMiddleware` with the given `service`.
    pub fn new(service: S) -> Self {
        Self { service }
    }
}

impl<'a, S> RpcServiceT<'a> for RpcMetricsMiddleware<S>
where
    S: RpcServiceT<'a> + Send + Sync + Clone + 'static,
{
    type Future = ResponseFuture<futures::future::BoxFuture<'a, MethodResponse>>;

    fn call(&self, request: jsonrpsee::types::Request<'a>) -> Self::Future {
        let service = self.service.clone();
        let method = request.method_name().to_owned();
        let start = Instant::now();

        // Increment active requests gauge
        metrics::gauge!("rpc.active_requests").increment(1.0);

        ResponseFuture::future(Box::pin(async move {
            let response = service.call(request).await;
            let duration = start.elapsed().as_secs_f64();

            // Determine status and record metrics
            let status = if response.is_error() {
                "error"
            } else {
                "success"
            };

            // Record request count
            metrics::counter!("rpc.requests.total", "method" => method.clone(), "status" => status)
                .increment(1);

            // Record request duration
            metrics::histogram!("rpc.request.duration_seconds", "method" => method.clone())
                .record(duration);

            // Record errors with error code
            if response.is_error() {
                let error_code = response
                    .as_error_code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                metrics::counter!(
                    "rpc.errors.total",
                    "method" => method,
                    "error_code" => error_code
                )
                .increment(1);
            }

            // Decrement active requests gauge
            metrics::gauge!("rpc.active_requests").decrement(1.0);

            response
        }))
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

    #[derive(Clone, Debug)]
    struct PendingRpcService;

    impl<'a> RpcServiceT<'a> for PendingRpcService {
        type Future = future::Pending<MethodResponse>;

        fn call(&self, _request: Request<'a>) -> Self::Future {
            future::pending()
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct RecordedMetric {
        name: String,
        labels: Vec<(String, String)>,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct RecordedGaugeEvent {
        name: String,
        labels: Vec<(String, String)>,
        operation: GaugeOperation,
        value: f64,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    enum GaugeOperation {
        Increment,
        Decrement,
        Set,
    }

    #[derive(Default)]
    struct RecordingRecorder {
        metrics: Mutex<Vec<RecordedMetric>>,
        gauge_events: Arc<Mutex<Vec<RecordedGaugeEvent>>>,
    }

    impl RecordingRecorder {
        fn metric_from_key(key: &metrics::Key) -> RecordedMetric {
            RecordedMetric {
                name: key.name().to_string(),
                labels: key
                    .labels()
                    .map(|label| (label.key().to_string(), label.value().to_string()))
                    .collect(),
            }
        }

        fn record_key(&self, key: &metrics::Key) {
            self.metrics
                .lock()
                .expect("recorder mutex should not be poisoned because tests do not panic while holding it")
                .push(Self::metric_from_key(key));
        }

        fn recorded_metrics(&self) -> Vec<RecordedMetric> {
            self.metrics
                .lock()
                .expect("recorder mutex should not be poisoned because tests do not panic while holding it")
                .clone()
        }

        fn recorded_gauge_events(&self) -> Vec<RecordedGaugeEvent> {
            self.gauge_events
                .lock()
                .expect("recorder mutex should not be poisoned because tests do not panic while holding it")
                .clone()
        }
    }

    struct RecordingGauge {
        metric: RecordedMetric,
        gauge_events: Arc<Mutex<Vec<RecordedGaugeEvent>>>,
    }

    impl RecordingGauge {
        fn record(&self, operation: GaugeOperation, value: f64) {
            self.gauge_events
                .lock()
                .expect("recorder mutex should not be poisoned because tests do not panic while holding it")
                .push(RecordedGaugeEvent {
                    name: self.metric.name.clone(),
                    labels: self.metric.labels.clone(),
                    operation,
                    value,
                });
        }
    }

    impl metrics::GaugeFn for RecordingGauge {
        fn increment(&self, value: f64) {
            self.record(GaugeOperation::Increment, value);
        }

        fn decrement(&self, value: f64) {
            self.record(GaugeOperation::Decrement, value);
        }

        fn set(&self, value: f64) {
            self.record(GaugeOperation::Set, value);
        }
    }

    impl metrics::Recorder for RecordingRecorder {
        fn describe_counter(
            &self,
            _key: metrics::KeyName,
            _unit: Option<metrics::Unit>,
            _description: metrics::SharedString,
        ) {
        }

        fn describe_gauge(
            &self,
            _key: metrics::KeyName,
            _unit: Option<metrics::Unit>,
            _description: metrics::SharedString,
        ) {
        }

        fn describe_histogram(
            &self,
            _key: metrics::KeyName,
            _unit: Option<metrics::Unit>,
            _description: metrics::SharedString,
        ) {
        }

        fn register_counter(
            &self,
            key: &metrics::Key,
            _metadata: &metrics::Metadata<'_>,
        ) -> metrics::Counter {
            self.record_key(key);
            metrics::Counter::noop()
        }

        fn register_gauge(
            &self,
            key: &metrics::Key,
            _metadata: &metrics::Metadata<'_>,
        ) -> metrics::Gauge {
            self.record_key(key);
            metrics::Gauge::from_arc(Arc::new(RecordingGauge {
                metric: Self::metric_from_key(key),
                gauge_events: self.gauge_events.clone(),
            }))
        }

        fn register_histogram(
            &self,
            key: &metrics::Key,
            _metadata: &metrics::Metadata<'_>,
        ) -> metrics::Histogram {
            self.record_key(key);
            metrics::Histogram::noop()
        }
    }

    fn metric_has_label(
        metrics: &[RecordedMetric],
        metric_name: &str,
        label_name: &str,
        label_value: &str,
    ) -> bool {
        metrics.iter().any(|metric| {
            metric.name == metric_name
                && metric
                    .labels
                    .iter()
                    .any(|(name, value)| name == label_name && value == label_value)
        })
    }

    fn gauge_event_exists(
        events: &[RecordedGaugeEvent],
        metric_name: &str,
        operation: GaugeOperation,
        value: f64,
    ) -> bool {
        events.iter().any(|event| {
            event.name == metric_name && event.operation == operation && event.value == value
        })
    }

    #[test]
    fn rpc_metrics_method_label_uses_raw_unknown_method_today() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime should build for this metrics capture test");
        let recorder = RecordingRecorder::default();
        let service = RpcMetricsMiddleware::new(SuccessRpcService);
        let unknown_methods = [
            "unknown_cardinality_probe_method_a",
            "unknown_cardinality_probe_method_b",
        ];

        metrics::with_local_recorder(&recorder, || {
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

        let metrics = recorder.recorded_metrics();

        for method in unknown_methods {
            assert!(
                metric_has_label(&metrics, "rpc.requests.total", "method", method),
                "request counter should use the raw unknown method as a Prometheus label: {metrics:?}"
            );
            assert!(
                metric_has_label(&metrics, "rpc.request.duration_seconds", "method", method),
                "duration histogram should use the raw unknown method as a Prometheus label: {metrics:?}"
            );
        }
    }

    #[test]
    fn rpc_active_requests_gauge_not_decremented_when_response_future_dropped_today() {
        let recorder = RecordingRecorder::default();
        let service = RpcMetricsMiddleware::new(PendingRpcService);

        metrics::with_local_recorder(&recorder, || {
            let response_future = service.call(Request::new(
                Cow::Borrowed("long_pending_method"),
                None,
                Id::Number(1),
            ));

            drop(response_future);
        });

        let gauge_events = recorder.recorded_gauge_events();

        assert!(
            gauge_event_exists(
                &gauge_events,
                "rpc.active_requests",
                GaugeOperation::Increment,
                1.0
            ),
            "middleware should increment active requests before returning the response future: {gauge_events:?}"
        );
        assert!(
            !gauge_event_exists(
                &gauge_events,
                "rpc.active_requests",
                GaugeOperation::Decrement,
                1.0
            ),
            "dropping the pending response future should not reach the current decrement path: {gauge_events:?}"
        );
    }
}
