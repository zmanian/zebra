//! Implements methods for testing [`Handshake`]

#![allow(clippy::unwrap_in_result)]

use super::*;

use std::sync::Mutex;

use zebra_chain::{block::Block, serialization::ZcashDeserializeInto};

#[derive(Clone, Debug, PartialEq)]
struct RecordedMetric {
    name: String,
    labels: Vec<(String, String)>,
}

#[derive(Default)]
struct RecordingRecorder {
    metrics: Mutex<Vec<RecordedMetric>>,
}

impl RecordingRecorder {
    fn record_key(&self, key: &metrics::Key) {
        self.metrics
            .lock()
            .expect(
                "recorder mutex should not be poisoned because tests do not panic while holding it",
            )
            .push(RecordedMetric {
                name: key.name().to_string(),
                labels: key
                    .labels()
                    .map(|label| (label.key().to_string(), label.value().to_string()))
                    .collect(),
            });
    }

    fn recorded_metrics(&self) -> Vec<RecordedMetric> {
        self.metrics
            .lock()
            .expect(
                "recorder mutex should not be poisoned because tests do not panic while holding it",
            )
            .clone()
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
        metrics::Gauge::noop()
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

impl<S, C> Handshake<S, C>
where
    S: Service<Request, Response = Response, Error = BoxError> + Clone + Send + 'static,
    S::Future: Send,
    C: ChainTip + Clone + Send + 'static,
{
    /// Returns a count of how many connection nonces are stored in this [`Handshake`]
    pub async fn nonce_count(&self) -> usize {
        self.nonces.lock().await.len()
    }
}

#[test]
fn handshake_connected_metric_uses_remote_user_agent_label_today() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime should build for this metrics capture test");
    let recorder = RecordingRecorder::default();
    let remote_user_agents = [
        "/attack-cardinality-peer-a:0.0.0/",
        "/attack-cardinality-peer-b:0.0.0/",
    ];

    let metrics = metrics::with_local_recorder(&recorder, || {
        runtime.block_on(async {
            for (index, user_agent) in remote_user_agents.iter().enumerate() {
                negotiate_with_remote_user_agent(user_agent, index as u16).await;
            }
        });

        recorder.recorded_metrics()
    });

    for user_agent in remote_user_agents {
        assert!(
            metric_has_label(
                &metrics,
                "zcash.net.peers.connected",
                "user_agent",
                user_agent,
            ),
            "connected peer metric should use the raw remote user agent as a label"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn handshake_decodes_and_ignores_pre_version_block_today() {
    let network = Network::Mainnet;
    let listen_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), network.default_port());
    let remote_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 31_000);
    let config = Config {
        listen_addr,
        network: network.clone(),
        ..Config::default()
    };
    let connected_addr = ConnectedAddr::new_inbound_direct(PeerSocketAddr::from(remote_addr));
    let nonces = Arc::new(futures::lock::Mutex::new(IndexSet::new()));
    let minimum_peer_version = MinimumPeerVersion::new(NoChainTip, &network);
    let (zebra_stream, remote_stream) = tokio::io::duplex(64 * 1024);
    let mut zebra_conn = Framed::new(
        zebra_stream,
        Codec::builder().for_network(&network).finish(),
    );
    let mut remote_conn = Framed::new(
        remote_stream,
        Codec::builder().for_network(&network).finish(),
    );

    let unsolicited_block: Block = zebra_test::vectors::BLOCK_MAINNET_1_BYTES
        .zcash_deserialize_into()
        .expect("test vector block should deserialize");

    let remote_task = tokio::spawn(async move {
        assert!(
            matches!(
                remote_conn
                    .next()
                    .await
                    .expect("Zebra should send a version message")
                    .expect("Zebra's version message should deserialize"),
                Message::Version(_)
            ),
            "Zebra should send version before verack"
        );

        remote_conn
            .send(Message::Block(Arc::new(unsolicited_block)))
            .await
            .expect("pre-version block message should serialize");

        let remote_version = VersionMessage {
            version: constants::CURRENT_NETWORK_PROTOCOL_VERSION,
            services: PeerServices::NODE_NETWORK,
            timestamp: Utc::now(),
            address_recv: AddrInVersion::new(listen_addr, PeerServices::NODE_NETWORK),
            address_from: AddrInVersion::new(remote_addr, PeerServices::NODE_NETWORK),
            nonce: Nonce::default(),
            user_agent: "/pre-version-block-test:0.0.0/".to_string(),
            start_height: zebra_chain::block::Height(0),
            relay: true,
        };

        remote_conn
            .send(remote_version.into())
            .await
            .expect("remote version message should serialize");

        assert!(
            matches!(
                remote_conn
                    .next()
                    .await
                    .expect("Zebra should send a verack message")
                    .expect("Zebra's verack message should deserialize"),
                Message::Verack
            ),
            "Zebra should send verack after accepting the remote version"
        );

        remote_conn
            .send(Message::Verack)
            .await
            .expect("remote verack message should serialize");
    });

    negotiate_version(
        &mut zebra_conn,
        &connected_addr,
        config,
        nonces,
        "local zebra test agent".to_string(),
        PeerServices::NODE_NETWORK,
        true,
        minimum_peer_version,
    )
    .await
    .expect("handshake should succeed after ignoring the pre-version block");

    remote_task
        .await
        .expect("remote side of in-memory handshake should not panic");
}

#[tokio::test]
async fn heartbeat_full_server_tx_times_out_and_reports_error_today() {
    let _init_guard = zebra_test::init();

    tokio::time::pause();

    let (mut server_tx, _server_rx) = futures::channel::mpsc::channel(0);
    let (address_book_updater, mut address_book_rx) = tokio::sync::mpsc::channel(1);
    let connected_addr = ConnectedAddr::new_inbound_direct(
        "127.0.0.1:8233"
            .parse()
            .expect("test peer address should parse"),
    );

    let heartbeat = heartbeat_timeout(
        send_one_heartbeat(&mut server_tx),
        &address_book_updater,
        &connected_addr,
    );
    tokio::pin!(heartbeat);

    assert!(matches!(futures::poll!(&mut heartbeat), Poll::Pending));

    tokio::time::advance(constants::HEARTBEAT_INTERVAL).await;

    assert!(
        heartbeat.await.is_err(),
        "a full heartbeat request channel should time out rather than dropping the heartbeat"
    );
    assert!(
        address_book_rx.try_recv().is_ok(),
        "heartbeat timeout should report the errored peer to the address book updater"
    );
}

async fn negotiate_with_remote_user_agent(remote_user_agent: &str, port_offset: u16) {
    let network = Network::Mainnet;
    let listen_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), network.default_port());
    let remote_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 30_000 + port_offset);
    let config = Config {
        listen_addr,
        network: network.clone(),
        ..Config::default()
    };
    let connected_addr = ConnectedAddr::new_inbound_direct(PeerSocketAddr::from(remote_addr));
    let nonces = Arc::new(futures::lock::Mutex::new(IndexSet::new()));
    let minimum_peer_version = MinimumPeerVersion::new(NoChainTip, &network);
    let (zebra_stream, remote_stream) = tokio::io::duplex(4 * 1024);
    let mut zebra_conn = Framed::new(
        zebra_stream,
        Codec::builder().for_network(&network).finish(),
    );
    let mut remote_conn = Framed::new(
        remote_stream,
        Codec::builder().for_network(&network).finish(),
    );

    let remote_user_agent = remote_user_agent.to_string();
    let remote_task = tokio::spawn(async move {
        assert!(
            matches!(
                remote_conn
                    .next()
                    .await
                    .expect("Zebra should send a version message")
                    .expect("Zebra's version message should deserialize"),
                Message::Version(_)
            ),
            "Zebra should send version before verack"
        );

        let remote_version = VersionMessage {
            version: constants::CURRENT_NETWORK_PROTOCOL_VERSION,
            services: PeerServices::NODE_NETWORK,
            timestamp: Utc::now(),
            address_recv: AddrInVersion::new(listen_addr, PeerServices::NODE_NETWORK),
            address_from: AddrInVersion::new(remote_addr, PeerServices::NODE_NETWORK),
            nonce: Nonce::default(),
            user_agent: remote_user_agent,
            start_height: zebra_chain::block::Height(0),
            relay: true,
        };

        remote_conn
            .send(remote_version.into())
            .await
            .expect("remote version message should serialize");

        assert!(
            matches!(
                remote_conn
                    .next()
                    .await
                    .expect("Zebra should send a verack message")
                    .expect("Zebra's verack message should deserialize"),
                Message::Verack
            ),
            "Zebra should send verack after accepting the remote version"
        );

        remote_conn
            .send(Message::Verack)
            .await
            .expect("remote verack message should serialize");
    });

    negotiate_version(
        &mut zebra_conn,
        &connected_addr,
        config,
        nonces,
        "local zebra test agent".to_string(),
        PeerServices::NODE_NETWORK,
        true,
        minimum_peer_version,
    )
    .await
    .expect("current-protocol peer handshake should succeed");

    remote_task
        .await
        .expect("remote side of in-memory handshake should not panic");
}
