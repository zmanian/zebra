//! Fixed test cases for batch worker tasks.

use std::time::Duration;

use tokio_test::{assert_pending, assert_ready, assert_ready_err, task};
use tower::{Service, ServiceExt};
use tower_batch_control::{error, Batch, BatchControl, RequestWeight};
use tower_test::mock;

#[derive(Clone, Debug)]
struct WeightedRequest(usize);

impl RequestWeight for WeightedRequest {
    fn request_weight(&self) -> usize {
        self.0
    }
}

#[tokio::test]
async fn wakes_pending_waiters_on_close() {
    let _init_guard = zebra_test::init();

    let (service, mut handle) = mock::pair::<_, ()>();

    let (mut service, worker) = Batch::pair(service, 1, 1, Duration::from_secs(1));
    let mut worker = task::spawn(worker.run());

    // // keep the request in the worker
    handle.allow(0);
    let service1 = service.ready().await.unwrap();
    let poll = worker.poll();
    assert_pending!(poll);
    let mut response = task::spawn(service1.call(()));

    let mut service1 = service.clone();
    let mut ready1 = task::spawn(service1.ready());
    assert_pending!(worker.poll());
    assert_pending!(ready1.poll(), "no capacity");

    let mut service1 = service.clone();
    let mut ready2 = task::spawn(service1.ready());
    assert_pending!(worker.poll());
    assert_pending!(ready2.poll(), "no capacity");

    // kill the worker task
    drop(worker);

    let err = assert_ready_err!(response.poll());
    assert!(
        err.is::<error::Closed>(),
        "response should fail with a Closed, got: {err:?}",
    );

    assert!(
        ready1.is_woken(),
        "dropping worker should wake ready task 1",
    );
    let err = assert_ready_err!(ready1.poll());
    assert!(
        err.is::<error::ServiceError>(),
        "ready 1 should fail with a ServiceError {{ Closed }}, got: {err:?}",
    );

    assert!(
        ready2.is_woken(),
        "dropping worker should wake ready task 2",
    );
    let err = assert_ready_err!(ready1.poll());
    assert!(
        err.is::<error::ServiceError>(),
        "ready 2 should fail with a ServiceError {{ Closed }}, got: {err:?}",
    );
}

#[tokio::test]
async fn wakes_pending_waiters_on_failure() {
    let _init_guard = zebra_test::init();

    let (service, mut handle) = mock::pair::<_, ()>();

    let (mut service, worker) = Batch::pair(service, 1, 1, Duration::from_secs(1));
    let mut worker = task::spawn(worker.run());

    // keep the request in the worker
    handle.allow(0);
    let service1 = service.ready().await.unwrap();
    assert_pending!(worker.poll());
    let mut response = task::spawn(service1.call("hello"));

    let mut service1 = service.clone();
    let mut ready1 = task::spawn(service1.ready());
    assert_pending!(worker.poll());
    assert_pending!(ready1.poll(), "no capacity");

    let mut service1 = service.clone();
    let mut ready2 = task::spawn(service1.ready());
    assert_pending!(worker.poll());
    assert_pending!(ready2.poll(), "no capacity");

    // fail the inner service
    handle.send_error("foobar");
    // worker task terminates
    assert_ready!(worker.poll());

    let err = assert_ready_err!(response.poll());
    assert!(
        err.is::<error::ServiceError>(),
        "response should fail with a ServiceError, got: {err:?}"
    );

    assert!(
        ready1.is_woken(),
        "dropping worker should wake ready task 1"
    );
    let err = assert_ready_err!(ready1.poll());
    assert!(
        err.is::<error::ServiceError>(),
        "ready 1 should fail with a ServiceError, got: {err:?}"
    );

    assert!(
        ready2.is_woken(),
        "dropping worker should wake ready task 2"
    );
    let err = assert_ready_err!(ready1.poll());
    assert!(
        err.is::<error::ServiceError>(),
        "ready 2 should fail with a ServiceError, got: {err:?}"
    );
}

#[tokio::test]
async fn weighted_requests_only_consume_one_queue_permit_today() {
    let _init_guard = zebra_test::init();

    let (service, _handle) = mock::pair::<BatchControl<WeightedRequest>, ()>();
    let (service, _worker) = Batch::pair(service, 2, 1, Duration::from_secs(1));

    let mut first_service = service.clone();
    let first_ready = first_service
        .ready()
        .await
        .expect("first full-weight request should get a queue permit");
    let _first_response = first_ready.call(WeightedRequest(2));

    let mut second_service = service.clone();
    let second_ready = second_service
        .ready()
        .await
        .expect("second full-weight request also gets a queue permit today");
    let _second_response = second_ready.call(WeightedRequest(2));

    let mut third_service = service.clone();
    let mut third_ready = task::spawn(third_service.ready());
    assert_pending!(
        third_ready.poll(),
        "queue capacity is exhausted after two requests, not after one full-weight request"
    );
}
