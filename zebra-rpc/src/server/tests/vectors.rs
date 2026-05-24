//! Fixed test vectors for the RPC server.

// These tests call functions which can take unit arguments if some features aren't enabled.
#![allow(clippy::unit_arg)]

use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream as TokioTcpStream,
    sync::watch,
};
use tower::buffer::Buffer;

use zebra_chain::{
    chain_sync_status::MockSyncStatus, chain_tip::NoChainTip, parameters::Network::*,
};
use zebra_network::address_book_peers::MockAddressBookPeers;
use zebra_node_services::BoxError;
use zebra_test::mock_service::MockService;

use super::super::*;

use config::rpc::Config;

/// Test that the JSON-RPC server spawns.
#[tokio::test]
async fn rpc_server_spawn_test() {
    rpc_server_spawn().await
}

/// Test if the RPC server will spawn on a randomly generated port.
#[tracing::instrument]
async fn rpc_server_spawn() {
    let _init_guard = zebra_test::init();

    let conf = Config {
        listen_addr: Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0).into()),
        indexer_listen_addr: None,
        parallel_cpu_threads: 0,
        debug_force_finished_sync: false,
        cookie_dir: Default::default(),
        enable_cookie_auth: false,
        max_response_body_size: Default::default(),
    };

    let mut mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    info!("spawning RPC server...");

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool.clone(), 1),
        Buffer::new(state.clone(), 1),
        Buffer::new(read_state.clone(), 1),
        Buffer::new(block_verifier_router.clone(), 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx,
        None,
    );

    RpcServer::start(rpc_impl, conf)
        .await
        .expect("RPC server should start");

    info!("spawned RPC server, checking services...");

    mempool.expect_no_requests().await;
    read_state.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;
}

/// Test that the JSON-RPC server spawns on an OS-assigned unallocated port.
#[tokio::test]
async fn rpc_server_spawn_unallocated_port() {
    rpc_spawn_unallocated_port(false).await
}

/// Test that the JSON-RPC server spawns and shuts down on an OS-assigned unallocated port.
#[tokio::test]
async fn rpc_server_spawn_unallocated_port_shutdown() {
    rpc_spawn_unallocated_port(true).await
}

/// Test if the RPC server will spawn on an OS-assigned unallocated port.
///
/// Set `do_shutdown` to true to close the server using the close handle.
#[tracing::instrument]
async fn rpc_spawn_unallocated_port(do_shutdown: bool) {
    let _init_guard = zebra_test::init();

    let port = zebra_test::net::random_unallocated_port();
    #[allow(unknown_lints)]
    #[allow(clippy::bool_to_int_with_if)]
    let conf = Config {
        listen_addr: Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into()),
        indexer_listen_addr: None,
        parallel_cpu_threads: 0,
        debug_force_finished_sync: false,
        cookie_dir: Default::default(),
        enable_cookie_auth: false,
        max_response_body_size: Default::default(),
    };

    let mut mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    info!("spawning RPC server...");

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool.clone(), 1),
        Buffer::new(state.clone(), 1),
        Buffer::new(read_state.clone(), 1),
        Buffer::new(block_verifier_router.clone(), 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx,
        None,
    );

    let rpc = RpcServer::start(rpc_impl, conf)
        .await
        .expect("server should start");

    info!("spawned RPC server, checking services...");

    mempool.expect_no_requests().await;
    state.expect_no_requests().await;
    read_state.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    if do_shutdown {
        rpc.abort();
    }
}

/// Test that incomplete HTTP bodies stay open at the real RPC server boundary
/// before method dispatch.
#[tokio::test]
async fn rpc_server_incomplete_body_waits_before_dispatch_today() {
    let _init_guard = zebra_test::init();

    let port = zebra_test::net::random_known_port();
    let listen_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let conf = Config {
        listen_addr: Some(listen_addr.into()),
        indexer_listen_addr: None,
        parallel_cpu_threads: 0,
        debug_force_finished_sync: false,
        cookie_dir: Default::default(),
        enable_cookie_auth: false,
        max_response_body_size: Default::default(),
    };

    let mut mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool.clone(), 1),
        Buffer::new(state.clone(), 1),
        Buffer::new(read_state.clone(), 1),
        Buffer::new(block_verifier_router.clone(), 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx,
        None,
    );

    let rpc = RpcServer::start(rpc_impl, conf)
        .await
        .expect("server should start");

    let body = r#"{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}"#;
    let request = format!(
        "POST / HTTP/1.1\r\n\
         Host: {listen_addr}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {body}",
        body.len() + 1024
    );

    let mut streams = Vec::new();

    for _ in 0..3 {
        let mut stream =
            tokio::time::timeout(Duration::from_secs(5), TokioTcpStream::connect(listen_addr))
                .await
                .expect("test RPC connection should not time out")
                .expect("test RPC connection should succeed");

        stream
            .write_all(request.as_bytes())
            .await
            .expect("test should write partial request body");

        streams.push(stream);
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    for stream in &mut streams {
        let mut response_byte = [0u8; 1];
        let result =
            tokio::time::timeout(Duration::from_millis(10), stream.read(&mut response_byte)).await;

        assert!(
            result.is_err(),
            "incomplete request body should not get an RPC response or close today"
        );
    }

    mempool.expect_no_requests().await;
    state.expect_no_requests().await;
    read_state.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    drop(streams);
    rpc.abort();
    let _ = rpc.await;
}

/// Test that the real RPC server stack applies Zebra's HTTP compatibility
/// middleware before jsonrpsee dispatch.
#[tokio::test]
async fn rpc_server_text_plain_strict_json_rpc_reaches_dispatch_today() {
    let _init_guard = zebra_test::init();

    let port = zebra_test::net::random_known_port();
    let listen_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let conf = Config {
        listen_addr: Some(listen_addr.into()),
        indexer_listen_addr: None,
        parallel_cpu_threads: 0,
        debug_force_finished_sync: false,
        cookie_dir: Default::default(),
        enable_cookie_auth: false,
        max_response_body_size: Default::default(),
    };

    let mut mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool.clone(), 1),
        Buffer::new(state.clone(), 1),
        Buffer::new(read_state.clone(), 1),
        Buffer::new(block_verifier_router.clone(), 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx,
        None,
    );

    let rpc = RpcServer::start(rpc_impl, conf)
        .await
        .expect("server should start");

    let body = r#"{ "params" : [], "method" : "definitely_not_a_real_method", "id" : 1, "jsonrpc" : "2.0" }"#;
    let request = format!(
        "POST / HTTP/1.1\r\n\
         Host: {listen_addr}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Connection: close\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {body}",
        body.len()
    );

    let mut stream =
        tokio::time::timeout(Duration::from_secs(5), TokioTcpStream::connect(listen_addr))
            .await
            .expect("test RPC connection should not time out")
            .expect("test RPC connection should succeed");

    stream
        .write_all(request.as_bytes())
        .await
        .expect("test should write complete request body");

    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .expect("test RPC response should not time out")
        .expect("test should read RPC response");

    let response = String::from_utf8(response).expect("HTTP response should be valid UTF-8");
    let (_headers, body) = response.split_once("\r\n\r\n").unwrap_or_else(|| {
        panic!("HTTP response should contain a header/body separator: {response:?}")
    });
    let json: serde_json::Value = serde_json::from_str(body).expect("response body should be JSON");

    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 1);
    assert_eq!(json["error"]["code"], -32601);

    mempool.expect_no_requests().await;
    state.expect_no_requests().await;
    read_state.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    rpc.abort();
    let _ = rpc.await;
}

/// Test if the RPC server will panic correctly when there is a port conflict.
#[tokio::test]
async fn rpc_server_spawn_port_conflict() {
    use std::time::Duration;
    let _init_guard = zebra_test::init();

    let port = zebra_test::net::random_known_port();
    let conf = Config {
        listen_addr: Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into()),
        indexer_listen_addr: None,
        debug_force_finished_sync: false,
        parallel_cpu_threads: 0,
        cookie_dir: Default::default(),
        enable_cookie_auth: false,
        max_response_body_size: Default::default(),
    };

    let mut mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let mut block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool.clone(), 1),
        Buffer::new(state.clone(), 1),
        Buffer::new(read_state.clone(), 1),
        Buffer::new(block_verifier_router.clone(), 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx.clone(),
        None,
    );

    RpcServer::start(rpc_impl.clone(), conf.clone())
        .await
        .expect("RPC server should start");

    tokio::time::sleep(Duration::from_secs(3)).await;

    RpcServer::start(rpc_impl, conf)
        .await
        .expect_err("RPC server should not start");

    mempool.expect_no_requests().await;
    state.expect_no_requests().await;
    read_state.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;
}

/// Test that an auth-enabled RPC startup failure leaves the cookie on disk today.
#[tokio::test]
async fn rpc_server_start_failure_leaves_cookie_today() {
    let _init_guard = zebra_test::init();

    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .expect("test should reserve a local TCP port");
    let listen_addr = listener
        .local_addr()
        .expect("reserved local TCP listener should have an address");
    let cookie_dir = tempfile::tempdir().expect("test should create a temporary cookie dir");

    let conf = Config {
        listen_addr: Some(listen_addr),
        indexer_listen_addr: None,
        debug_force_finished_sync: false,
        parallel_cpu_threads: 0,
        cookie_dir: cookie_dir.path().to_path_buf(),
        enable_cookie_auth: true,
        max_response_body_size: Default::default(),
    };

    let mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool, 1),
        Buffer::new(state, 1),
        Buffer::new(read_state, 1),
        Buffer::new(block_verifier_router, 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx,
        None,
    );

    RpcServer::start(rpc_impl, conf)
        .await
        .expect_err("RPC server should not start while the port is reserved");

    assert!(
        cookie_dir.path().join(".cookie").exists(),
        "failed RPC startup leaves the auth cookie on disk today"
    );

    drop(listener);
}

/// Test that aborting the live auth-enabled RPC task leaves the cookie on disk today.
#[tokio::test]
async fn rpc_server_task_abort_leaves_cookie_today() {
    let _init_guard = zebra_test::init();

    let cookie_dir = tempfile::tempdir().expect("test should create a temporary cookie dir");
    let conf = Config {
        listen_addr: Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0).into()),
        indexer_listen_addr: None,
        parallel_cpu_threads: 0,
        debug_force_finished_sync: false,
        cookie_dir: cookie_dir.path().to_path_buf(),
        enable_cookie_auth: true,
        max_response_body_size: Default::default(),
    };

    let mempool: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let read_state: MockService<_, _, _, BoxError> = MockService::build().for_unit_tests();
    let block_verifier_router: MockService<_, _, _, BoxError> =
        MockService::build().for_unit_tests();

    let (_tx, rx) = watch::channel(None);
    let (rpc_impl, _) = RpcImpl::new(
        Mainnet,
        Default::default(),
        false,
        "RPC test",
        "RPC test",
        Buffer::new(mempool, 1),
        Buffer::new(state, 1),
        Buffer::new(read_state, 1),
        Buffer::new(block_verifier_router, 1),
        MockSyncStatus::default(),
        NoChainTip,
        MockAddressBookPeers::default(),
        rx,
        None,
    );

    let rpc_task = RpcServer::start(rpc_impl, conf)
        .await
        .expect("RPC server should start on an OS-assigned port");

    assert!(
        cookie_dir.path().join(".cookie").exists(),
        "auth-enabled RPC startup should write the cookie"
    );

    rpc_task.abort();
    let _ = rpc_task.await;

    assert!(
        cookie_dir.path().join(".cookie").exists(),
        "aborted RPC server task leaves the auth cookie on disk today"
    );
}
