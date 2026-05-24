//! Fixed test vectors for indexer RPCs

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{FutureExt, StreamExt};
use tokio::{sync::broadcast, task::JoinHandle};
use tower::BoxError;
use zebra_chain::{
    block::{self, Height},
    chain_tip::{
        mock::{MockChainTip, MockChainTipSender},
        ChainTip,
    },
    parameters::Network,
    serialization::{BytesInDisplayOrder, ZcashDeserializeInto},
    transaction::{self, AuthDigest, UnminedTxId, WtxId},
};
use zebra_node_services::mempool::{MempoolChange, MempoolTxSubscriber};
use zebra_test::{
    mock_service::{MockService, PanicAssertion},
    prelude::color_eyre::{eyre::eyre, Result},
};

use crate::indexer::{self, indexer_client::IndexerClient, indexer_server::Indexer, Empty};

#[derive(Clone)]
struct ProbeChainTip {
    receiver: tokio::sync::watch::Receiver<u64>,
    tip: Arc<Mutex<Option<(Height, block::Hash)>>>,
    started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    dropped: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

struct ProbeChainTipController {
    sender: tokio::sync::watch::Sender<u64>,
    tip: Arc<Mutex<Option<(Height, block::Hash)>>>,
}

struct BestTipChangedDropProbe(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for BestTipChangedDropProbe {
    fn drop(&mut self) {
        if let Some(dropped) = self.0.take() {
            let _ = dropped.send(());
        }
    }
}

impl ProbeChainTip {
    fn new() -> (
        Self,
        ProbeChainTipController,
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Receiver<()>,
    ) {
        let (sender, receiver) = tokio::sync::watch::channel(0);
        let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
        let (dropped_sender, dropped_receiver) = tokio::sync::oneshot::channel();
        let tip = Arc::new(Mutex::new(None));

        (
            Self {
                receiver,
                tip: tip.clone(),
                started: Arc::new(Mutex::new(Some(started_sender))),
                dropped: Arc::new(Mutex::new(Some(dropped_sender))),
            },
            ProbeChainTipController { sender, tip },
            started_receiver,
            dropped_receiver,
        )
    }
}

impl ProbeChainTipController {
    fn send_best_tip(&self, height: Height, hash: block::Hash) {
        *self
            .tip
            .lock()
            .expect("probe chain tip mutex should not be poisoned because tests do not panic while holding it") =
            Some((height, hash));
        let next_change = *self.sender.borrow() + 1;
        self.sender
            .send(next_change)
            .expect("probe chain tip receiver should still be live");
    }
}

impl ChainTip for ProbeChainTip {
    fn best_tip_height(&self) -> Option<Height> {
        self.tip
            .lock()
            .expect("probe chain tip mutex should not be poisoned because tests do not panic while holding it")
            .map(|(height, _hash)| height)
    }

    fn best_tip_hash(&self) -> Option<block::Hash> {
        self.tip
            .lock()
            .expect("probe chain tip mutex should not be poisoned because tests do not panic while holding it")
            .map(|(_height, hash)| hash)
    }

    fn best_tip_height_and_hash(&self) -> Option<(Height, block::Hash)> {
        *self
            .tip
            .lock()
            .expect("probe chain tip mutex should not be poisoned because tests do not panic while holding it")
    }

    fn best_tip_block_time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None
    }

    fn best_tip_height_and_block_time(&self) -> Option<(Height, chrono::DateTime<chrono::Utc>)> {
        None
    }

    fn best_tip_mined_transaction_ids(&self) -> Arc<[transaction::Hash]> {
        Arc::new([])
    }

    async fn best_tip_changed(&mut self) -> Result<(), BoxError> {
        if let Some(started) = self
            .started
            .lock()
            .expect("probe chain tip mutex should not be poisoned because tests do not panic while holding it")
            .take()
        {
            let _ = started.send(());
        }
        let _drop_probe = BestTipChangedDropProbe(
            self.dropped
                .lock()
                .expect("probe chain tip mutex should not be poisoned because tests do not panic while holding it")
                .take(),
        );

        self.receiver.changed().await?;

        Ok(())
    }

    fn mark_best_tip_seen(&mut self) {
        self.receiver.borrow_and_update();
    }

    fn estimate_distance_to_network_chain_tip(
        &self,
        _network: &Network,
    ) -> Option<(block::HeightDiff, Height)> {
        None
    }
}

#[tokio::test]
async fn rpc_server_spawn() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (_server_task, client, mock_chain_tip_sender, mempool_transaction_sender, _read_state) =
        start_server_and_get_client().await?;

    test_chain_tip_change(client.clone(), mock_chain_tip_sender).await?;
    test_mempool_change(client.clone(), mempool_transaction_sender).await?;

    Ok(())
}

#[tokio::test]
async fn indexer_accepts_many_unauthenticated_streams_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (server_task, client, _mock_chain_tip_sender, _mempool_transaction_sender, _read_state) =
        start_server_and_get_client().await?;

    let mut streams = Vec::new();
    for _ in 0..32 {
        let mut stream_client = client.clone();
        let stream = stream_client
            .chain_tip_change(tonic::Request::new(Empty {}))
            .await?
            .into_inner();
        streams.push(stream);
    }

    assert_eq!(
        streams.len(),
        32,
        "indexer accepts many unauthenticated streaming subscribers today",
    );

    drop(streams);
    server_task.abort();

    Ok(())
}

#[tokio::test]
async fn dropped_chain_tip_change_stream_retains_task_until_tip_event_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let read_state = MockService::build().for_unit_tests();
    let (mempool_transaction_sender, _) = tokio::sync::broadcast::channel(1);
    let (chain_tip, chain_tip_controller, started_receiver, mut dropped_receiver) =
        ProbeChainTip::new();
    let rpc = indexer::server::IndexerRPC::new_for_tests(
        read_state,
        chain_tip,
        MempoolTxSubscriber::new(mempool_transaction_sender),
    );

    let response = rpc
        .chain_tip_change(tonic::Request::new(Empty {}))
        .await?
        .into_inner();

    tokio::time::timeout(Duration::from_secs(3), started_receiver)
        .await
        .expect("chain_tip_change task should spawn before timeout")
        .expect("chain_tip_change task should signal startup before dropping its probe");

    drop(response);
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        (&mut dropped_receiver).now_or_never().is_none(),
        "dropping the client stream does not drop the idle chain_tip_change task today"
    );

    chain_tip_controller.send_best_tip(Height::MIN, block::Hash([0; 32]));

    tokio::time::timeout(Duration::from_secs(3), dropped_receiver)
        .await
        .expect("chain_tip_change task should exit after a tip event observes the closed stream")
        .expect("chain_tip_change task should drop its probe after a tip event");

    Ok(())
}

#[tokio::test]
async fn non_finalized_state_streams_request_one_listener_each_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (server_task, client, _mock_chain_tip_sender, _mempool_transaction_sender, mut read_state) =
        start_server_and_get_client().await?;

    let mut streams = Vec::new();
    for _ in 0..8 {
        let mut stream_client = client.clone();
        let stream = stream_client
            .non_finalized_state_change(tonic::Request::new(Empty {}))
            .await?
            .into_inner();
        streams.push(stream);
    }

    for _ in 0..streams.len() {
        let _ = read_state
            .expect_request(zebra_state::ReadRequest::NonFinalizedBlocksListener)
            .await;
    }

    drop(streams);
    server_task.abort();

    Ok(())
}

#[tokio::test]
async fn dropped_non_finalized_state_stream_retains_state_listener_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (
        server_task,
        mut client,
        _mock_chain_tip_sender,
        _mempool_transaction_sender,
        mut read_state,
    ) = start_server_and_get_client().await?;

    let response = client
        .non_finalized_state_change(tonic::Request::new(Empty {}))
        .await?
        .into_inner();

    let response_handler = read_state
        .expect_request(zebra_state::ReadRequest::NonFinalizedBlocksListener)
        .await;
    let (listener_sender, listener_receiver) = tokio::sync::mpsc::channel(1);
    response_handler.respond(zebra_state::ReadResponse::NonFinalizedBlocksListener(
        zebra_state::NonFinalizedBlocksListener(Arc::new(listener_receiver)),
    ));

    drop(response);
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        !listener_sender.is_closed(),
        "dropping the client stream does not drop the non_finalized_state_change listener today"
    );

    server_task.abort();

    Ok(())
}

#[tokio::test]
async fn mempool_change_stream_exposes_v5_auth_digest_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (server_task, mut client, _mock_chain_tip_sender, mempool_transaction_sender, _read_state) =
        start_server_and_get_client().await?;

    let mut response = client
        .mempool_change(tonic::Request::new(Empty {}))
        .await?
        .into_inner();

    let mined_id = transaction::Hash::from([0x11; 32]);
    let auth_digest = AuthDigest([0x22; 32]);
    let wtx_id = WtxId {
        id: mined_id,
        auth_digest,
    };
    let change_tx_ids = [UnminedTxId::Witnessed(wtx_id)].into_iter().collect();

    mempool_transaction_sender
        .send(MempoolChange::added(change_tx_ids))
        .expect("rpc server should have a receiver");

    let change = tokio::time::timeout(Duration::from_secs(3), response.next())
        .await
        .expect("should receive mempool change notification before timeout")
        .expect("response stream should not be empty")
        .expect("mempool change response should not be an error message");

    assert_eq!(change.change_type, 0);
    assert_eq!(change.tx_hash, mined_id.bytes_in_display_order().to_vec());
    assert_eq!(
        change.auth_digest,
        auth_digest.bytes_in_display_order().to_vec(),
        "MempoolChange streams V5 authorization digests today",
    );

    server_task.abort();

    Ok(())
}

#[tokio::test]
async fn lagged_mempool_change_stream_ends_as_unavailable_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (server_task, mut client, _mock_chain_tip_sender, mempool_transaction_sender, _read_state) =
        start_server_and_get_client().await?;

    let mut response = client
        .mempool_change(tonic::Request::new(Empty {}))
        .await?
        .into_inner();

    tokio::time::timeout(Duration::from_secs(3), async {
        while mempool_transaction_sender.receiver_count() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("mempool_change task should subscribe before timeout");

    let first_change_tx_ids = [UnminedTxId::Legacy(transaction::Hash::from([0x33; 32]))]
        .into_iter()
        .collect();
    let second_change_tx_ids = [UnminedTxId::Legacy(transaction::Hash::from([0x44; 32]))]
        .into_iter()
        .collect();

    mempool_transaction_sender
        .send(MempoolChange::added(first_change_tx_ids))
        .expect("rpc server should have a receiver");
    mempool_transaction_sender
        .send(MempoolChange::added(second_change_tx_ids))
        .expect("rpc server should have a receiver");

    let error = tokio::time::timeout(Duration::from_secs(3), response.next())
        .await
        .expect("should receive terminal mempool change status before timeout")
        .expect("response stream should not be empty")
        .expect_err("lagged mempool change stream exits with an error today");

    assert_eq!(
        error.code(),
        tonic::Code::Unavailable,
        "lagged mempool change stream is reported as unavailable today"
    );
    assert_eq!(
        error.message(),
        "mempool_change channel has closed",
        "lagged mempool change stream uses the same status message as channel closure today"
    );

    server_task.abort();

    Ok(())
}

#[tokio::test]
async fn dropped_mempool_change_stream_retains_subscription_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (server_task, mut client, _mock_chain_tip_sender, mempool_transaction_sender, _read_state) =
        start_server_and_get_client().await?;

    assert_eq!(mempool_transaction_sender.receiver_count(), 0);

    let response = client
        .mempool_change(tonic::Request::new(Empty {}))
        .await?
        .into_inner();

    tokio::time::timeout(Duration::from_secs(3), async {
        while mempool_transaction_sender.receiver_count() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("mempool_change task should subscribe before timeout");

    assert_eq!(mempool_transaction_sender.receiver_count(), 1);

    drop(response);
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        mempool_transaction_sender.receiver_count(),
        1,
        "dropping the client stream does not drop the mempool_change subscription today"
    );

    server_task.abort();

    Ok(())
}

#[test]
fn block_and_hash_decode_accepts_mismatched_hash_today() -> Result<()> {
    let block: block::Block = zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES
        .zcash_deserialize_into()
        .expect("genesis block test vector should deserialize");
    let actual_hash = block.hash();
    let transmitted_hash = block::Hash([0x5a; 32]);

    let message = indexer::BlockAndHash {
        hash: transmitted_hash.bytes_in_display_order().to_vec(),
        data: zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.to_vec(),
    };

    let (decoded_block, decoded_hash) = message
        .decode()
        .expect("mismatched hash and valid block bytes are accepted today");

    assert_eq!(decoded_block.hash(), actual_hash);
    assert_eq!(decoded_hash, transmitted_hash);
    assert_ne!(
        decoded_hash, actual_hash,
        "BlockAndHash::decode does not require the transmitted hash to match the block body today",
    );

    Ok(())
}

async fn test_chain_tip_change(
    mut client: IndexerClient<tonic::transport::Channel>,
    mock_chain_tip_sender: MockChainTipSender,
) -> Result<()> {
    let request = tonic::Request::new(Empty {});
    let mut response = client.chain_tip_change(request).await?.into_inner();
    mock_chain_tip_sender.send_best_tip_height(Height::MIN);
    mock_chain_tip_sender.send_best_tip_hash(zebra_chain::block::Hash([0; 32]));

    // Wait for RPC server to send a message
    tokio::time::sleep(Duration::from_millis(500)).await;

    tokio::time::timeout(Duration::from_secs(3), response.next())
        .await
        .expect("should receive chain tip change notification before timeout")
        .expect("response stream should not be empty")
        .expect("chain tip change response should not be an error message");

    Ok(())
}

async fn test_mempool_change(
    mut client: IndexerClient<tonic::transport::Channel>,
    mempool_transaction_sender: tokio::sync::broadcast::Sender<MempoolChange>,
) -> Result<()> {
    let request = tonic::Request::new(Empty {});
    let mut response = client.mempool_change(request).await?.into_inner();

    let change_tx_ids = [UnminedTxId::Legacy(transaction::Hash::from([0; 32]))]
        .into_iter()
        .collect();

    mempool_transaction_sender
        .send(MempoolChange::added(change_tx_ids))
        .expect("rpc server should have a receiver");

    tokio::time::timeout(Duration::from_secs(3), response.next())
        .await
        .expect("should receive chain tip change notification before timeout")
        .expect("response stream should not be empty")
        .expect("chain tip change response should not be an error message");

    Ok(())
}

async fn start_server_and_get_client() -> Result<(
    JoinHandle<Result<(), BoxError>>,
    IndexerClient<tonic::transport::Channel>,
    MockChainTipSender,
    broadcast::Sender<MempoolChange>,
    MockService<zebra_state::ReadRequest, zebra_state::ReadResponse, PanicAssertion, BoxError>,
)> {
    let listen_addr: std::net::SocketAddr = "127.0.0.1:0"
        .parse()
        .expect("hard-coded IP and u16 port should parse successfully");

    let mock_read_service = MockService::build()
        .with_max_request_delay(Duration::from_secs(2))
        .for_unit_tests();

    let (mock_chain_tip_change, mock_chain_tip_change_sender) = MockChainTip::new();
    let (mempool_transaction_sender, _) = tokio::sync::broadcast::channel(1);
    let mempool_tx_subscriber = MempoolTxSubscriber::new(mempool_transaction_sender.clone());
    let (server_task, listen_addr) = indexer::server::init(
        listen_addr,
        mock_read_service.clone(),
        mock_chain_tip_change,
        mempool_tx_subscriber.clone(),
    )
    .await
    .map_err(|err| eyre!(err))?;

    // wait for the server to start
    tokio::time::sleep(Duration::from_secs(1)).await;

    let endpoint = tonic::transport::channel::Endpoint::new(format!("http://{listen_addr}"))
        .unwrap()
        .timeout(Duration::from_secs(2));

    // connect to the gRPC server
    let client = IndexerClient::connect(endpoint)
        .await
        .expect("server should receive connection");

    Ok((
        server_task,
        client,
        mock_chain_tip_change_sender,
        mempool_transaction_sender,
        mock_read_service,
    ))
}
