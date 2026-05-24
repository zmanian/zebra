//! Syncer task for maintaining a non-finalized state in Zebra's ReadStateService and updating `ChainTipSender` via RPCs

use std::{net::SocketAddr, sync::Arc, time::Duration};

use tokio::task::JoinHandle;
use tonic::{Status, Streaming};
use tower::BoxError;
use zebra_chain::{block::Height, parameters::Network};
use zebra_state::{
    spawn_init_read_only, ChainTipBlock, ChainTipChange, ChainTipSender, CheckpointVerifiedBlock,
    LatestChainTip, NonFinalizedState, ReadStateService, SemanticallyVerifiedBlock,
    ValidateContextError, ZebraDb,
};

use zebra_chain::diagnostic::task::WaitForPanics;

use crate::indexer::{indexer_client::IndexerClient, BlockAndHash, Empty};

/// How long to wait between calls to `subscribe_to_non_finalized_state_change` when it returns an error.
const POLL_DELAY: Duration = Duration::from_secs(5);

/// Syncs non-finalized blocks in the best chain from a trusted Zebra node's RPC methods.
#[derive(Debug)]
pub struct TrustedChainSync {
    /// gRPC client for calling Zebra's indexer methods.
    pub indexer_rpc_client: IndexerClient<tonic::transport::Channel>,
    /// The read state service.
    db: ZebraDb,
    /// The non-finalized state - currently only contains the best chain.
    non_finalized_state: NonFinalizedState,
    /// The chain tip sender for updating [`LatestChainTip`] and [`ChainTipChange`].
    chain_tip_sender: ChainTipSender,
    /// The non-finalized state sender, for updating the [`ReadStateService`] when the non-finalized best chain changes.
    non_finalized_state_sender: tokio::sync::watch::Sender<NonFinalizedState>,
}

impl TrustedChainSync {
    /// Creates a new [`TrustedChainSync`] with a [`ChainTipSender`], then spawns a task to sync blocks
    /// from the node's non-finalized best chain.
    ///
    /// Returns the [`LatestChainTip`], [`ChainTipChange`], and a [`JoinHandle`] for the sync task.
    pub async fn spawn(
        indexer_rpc_address: SocketAddr,
        db: ZebraDb,
        non_finalized_state_sender: tokio::sync::watch::Sender<NonFinalizedState>,
    ) -> Result<(LatestChainTip, ChainTipChange, JoinHandle<()>), BoxError> {
        let non_finalized_state = NonFinalizedState::new(&db.network());
        let (chain_tip_sender, latest_chain_tip, chain_tip_change) =
            ChainTipSender::new(None, &db.network());
        let finalized_chain_tip_sender = chain_tip_sender.finalized_sender();
        let indexer_rpc_client =
            IndexerClient::connect(format!("http://{indexer_rpc_address}")).await?;

        let mut syncer = Self {
            indexer_rpc_client: indexer_rpc_client.clone(),
            db: db.clone(),
            non_finalized_state,
            chain_tip_sender,
            non_finalized_state_sender,
        };

        let finalized_tip_forwarder =
            spawn_finalized_tip_forwarder(indexer_rpc_client, db, finalized_chain_tip_sender);
        drop(finalized_tip_forwarder);

        let sync_task = tokio::spawn(async move {
            syncer.sync().await;
        });

        Ok((latest_chain_tip, chain_tip_change, sync_task))
    }

    /// Starts syncing blocks from the node's non-finalized best chain and checking for chain tip changes in the finalized state.
    ///
    /// When the best chain tip in Zebra is not available in the finalized state or the local non-finalized state,
    /// gets any unavailable blocks in Zebra's best chain from the RPC server, adds them to the local non-finalized state, then
    /// sends the updated chain tip block and non-finalized state to the [`ChainTipSender`] and non-finalized state sender.
    #[tracing::instrument(skip_all)]
    async fn sync(&mut self) {
        let mut non_finalized_blocks_listener = None;
        self.try_catch_up_with_primary().await;
        if let Some(finalized_tip_block) = self.finalized_chain_tip_block().await {
            self.chain_tip_sender.set_finalized_tip(finalized_tip_block);
        }

        loop {
            let Some(ref mut non_finalized_state_change) = non_finalized_blocks_listener else {
                non_finalized_blocks_listener = match self
                    .subscribe_to_non_finalized_state_change()
                    .await
                {
                    Ok(listener) => Some(listener),
                    Err(err) => {
                        tracing::warn!(?err, "failed to subscribe to non-finalized state changes");
                        tokio::time::sleep(POLL_DELAY).await;
                        None
                    }
                };

                continue;
            };

            let message = match non_finalized_state_change.message().await {
                Ok(Some(block_and_hash)) => block_and_hash,
                Ok(None) => {
                    tracing::warn!("non-finalized state change stream ended unexpectedly");
                    non_finalized_blocks_listener = None;
                    continue;
                }
                Err(err) => {
                    tracing::warn!(?err, "error receiving non-finalized state change");
                    non_finalized_blocks_listener = None;
                    continue;
                }
            };

            let Some((block, hash)) = message.decode() else {
                tracing::warn!("received malformed non-finalized state change message");
                non_finalized_blocks_listener = None;
                continue;
            };

            if self.non_finalized_state.any_chain_contains(&hash) {
                tracing::info!(?hash, "non-finalized state already contains block");
                continue;
            }

            let block = SemanticallyVerifiedBlock::with_hash(Arc::new(block), hash);
            match self.try_commit(block.clone()).await {
                Ok(()) => {
                    while self
                        .non_finalized_state
                        .root_height()
                        .expect("just successfully inserted a non-finalized block above")
                        <= self.db.finalized_tip_height().unwrap_or(Height::MIN)
                    {
                        tracing::trace!("finalizing block past the reorg limit");
                        self.non_finalized_state.finalize();
                    }

                    self.update_channels();
                }
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        ?hash,
                        "failed to commit block to non-finalized state"
                    );

                    // TODO: Investigate whether it would be correct to ignore some errors here instead of
                    //       trying every block in the non-finalized state again.
                    non_finalized_blocks_listener = None;
                }
            };
        }
    }

    async fn try_commit(
        &mut self,
        block: SemanticallyVerifiedBlock,
    ) -> Result<(), ValidateContextError> {
        self.try_catch_up_with_primary().await;

        if self.db.finalized_tip_hash() == block.block.header.previous_block_hash {
            self.non_finalized_state.commit_new_chain(block, &self.db)
        } else {
            self.non_finalized_state.commit_block(block, &self.db)
        }
    }

    /// Calls `non_finalized_state_change()` method on the indexer gRPC client to subscribe
    /// to non-finalized state changes, and returns the response stream.
    async fn subscribe_to_non_finalized_state_change(
        &mut self,
    ) -> Result<Streaming<BlockAndHash>, Status> {
        self.indexer_rpc_client
            .clone()
            .non_finalized_state_change(Empty {})
            .await
            .map(|a| a.into_inner())
    }

    /// Tries to catch up to the primary db instance for an up-to-date view of finalized blocks.
    async fn try_catch_up_with_primary(&self) {
        let _ = self.db.spawn_try_catch_up_with_primary().await;
    }

    /// Reads the finalized tip block from the secondary db instance and converts it to a [`ChainTipBlock`].
    async fn finalized_chain_tip_block(&self) -> Option<ChainTipBlock> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let (height, hash) = db.tip()?;
            db.block(height.into())
                .map(|block| CheckpointVerifiedBlock::with_hash(block, hash))
                .map(ChainTipBlock::from)
        })
        .wait_for_panics()
        .await
    }

    /// Sends the new chain tip and non-finalized state to the latest chain channels.
    // TODO: Replace this with the `update_latest_chain_channels()` fn in `write.rs`.
    fn update_channels(&mut self) {
        // If the final receiver was just dropped, ignore the error.
        let _ = self
            .non_finalized_state_sender
            .send(self.non_finalized_state.clone());

        let best_chain = self.non_finalized_state.best_chain().expect("unexpected empty non-finalized state: must commit at least one block before updating channels");

        let tip_block = best_chain
            .tip_block()
            .expect(
                "unexpected empty chain: must commit at least one block before updating channels",
            )
            .clone();

        self.chain_tip_sender
            .set_best_non_finalized_tip(Some(tip_block.into()));
    }
}

/// Accepts a [zebra-state configuration](zebra_state::Config), a [`Network`], and
/// the [`SocketAddr`] of a Zebra node's RPC server.
///
/// Initializes a [`ReadStateService`] and a [`TrustedChainSync`] to update the
/// non-finalized best chain and the latest chain tip.
///
/// Returns a [`ReadStateService`], [`LatestChainTip`], [`ChainTipChange`], and
/// a [`JoinHandle`] for the sync task.
pub fn init_read_state_with_syncer(
    config: zebra_state::Config,
    network: &Network,
    indexer_rpc_address: SocketAddr,
) -> tokio::task::JoinHandle<
    Result<
        (
            ReadStateService,
            LatestChainTip,
            ChainTipChange,
            tokio::task::JoinHandle<()>,
        ),
        BoxError,
    >,
> {
    let network = network.clone();
    tokio::spawn(async move {
        if config.ephemeral {
            return Err("standalone read state service cannot be used with ephemeral state".into());
        }

        let (read_state, db, non_finalized_state_sender) =
            spawn_init_read_only(config, &network).await?;
        let (latest_chain_tip, chain_tip_change, sync_task) =
            TrustedChainSync::spawn(indexer_rpc_address, db, non_finalized_state_sender).await?;
        Ok((read_state, latest_chain_tip, chain_tip_change, sync_task))
    })
}

/// Spawn a task to send finalized chain tip changes to the chain tip change and
/// latest chain tip channels.
fn spawn_finalized_tip_forwarder(
    mut indexer_rpc_client: IndexerClient<tonic::transport::Channel>,
    db: ZebraDb,
    mut finalized_chain_tip_sender: ChainTipSender,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut chain_tip_change_stream = None;

        loop {
            let Some(ref mut chain_tip_change) = chain_tip_change_stream else {
                chain_tip_change_stream = match indexer_rpc_client
                    .chain_tip_change(Empty {})
                    .await
                    .map(|a| a.into_inner())
                {
                    Ok(listener) => Some(listener),
                    Err(err) => {
                        tracing::warn!(?err, "failed to subscribe to non-finalized state changes");
                        tokio::time::sleep(POLL_DELAY).await;
                        None
                    }
                };

                continue;
            };

            let message = match chain_tip_change.message().await {
                Ok(Some(block_hash_and_height)) => block_hash_and_height,
                Ok(None) => {
                    tracing::warn!("chain_tip_change stream ended unexpectedly");
                    chain_tip_change_stream = None;
                    continue;
                }
                Err(err) => {
                    tracing::warn!(?err, "error receiving chain tip change");
                    chain_tip_change_stream = None;
                    continue;
                }
            };

            let Some((hash, _height)) = message.try_into_hash_and_height() else {
                tracing::warn!("failed to convert message into a block hash and height");
                continue;
            };

            // Skip the chain tip change if catching up to the primary db instance fails.
            if db.spawn_try_catch_up_with_primary().await.is_err() {
                continue;
            }

            // End the task and let the `TrustedChainSync::sync()` method send non-finalized chain tip updates if
            // the latest chain tip hash is not present in the db.
            let Some(tip_block) = db.block(hash.into()) else {
                return;
            };

            finalized_chain_tip_sender.set_finalized_tip(Some(
                SemanticallyVerifiedBlock::with_hash(tip_block, hash).into(),
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, sync::Arc};

    use futures::Stream;
    use tokio::sync::{mpsc, oneshot, Mutex};
    use tokio_stream::wrappers::ReceiverStream;
    use tonic::{
        transport::{server::TcpIncoming, Server},
        Request, Response,
    };
    use tower::BoxError;
    use zebra_chain::{
        block::{self, Block, Height},
        parameters::Network,
        serialization::ZcashDeserializeInto,
        transaction::{LockTime, Transaction},
        transparent,
    };
    use zebra_state::{Config, FinalizedState};

    use super::*;
    use crate::indexer::{
        indexer_server::{Indexer, IndexerServer},
        BlockHashAndHeight, MempoolChangeMessage,
    };

    #[derive(Clone)]
    struct TestIndexer {
        chain_tip_receiver: Arc<Mutex<Option<mpsc::Receiver<Result<BlockHashAndHeight, Status>>>>>,
        non_finalized_receiver: Arc<Mutex<Option<mpsc::Receiver<Result<BlockAndHash, Status>>>>>,
        mempool_receiver: Arc<Mutex<Option<mpsc::Receiver<Result<MempoolChangeMessage, Status>>>>>,
        chain_tip_subscribed: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    }

    #[tonic::async_trait]
    impl Indexer for TestIndexer {
        type ChainTipChangeStream =
            Pin<Box<dyn Stream<Item = Result<BlockHashAndHeight, Status>> + Send>>;
        type NonFinalizedStateChangeStream =
            Pin<Box<dyn Stream<Item = Result<BlockAndHash, Status>> + Send>>;
        type MempoolChangeStream =
            Pin<Box<dyn Stream<Item = Result<MempoolChangeMessage, Status>> + Send>>;

        async fn chain_tip_change(
            &self,
            _: Request<Empty>,
        ) -> Result<Response<Self::ChainTipChangeStream>, Status> {
            if let Some(subscribed) = self.chain_tip_subscribed.lock().await.take() {
                let _ = subscribed.send(());
            }

            let receiver = self
                .chain_tip_receiver
                .lock()
                .await
                .take()
                .expect("test only expects one chain_tip_change subscription");

            Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
        }

        async fn non_finalized_state_change(
            &self,
            _: Request<Empty>,
        ) -> Result<Response<Self::NonFinalizedStateChangeStream>, Status> {
            let receiver = self
                .non_finalized_receiver
                .lock()
                .await
                .take()
                .expect("test only expects one non_finalized_state_change subscription");

            Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
        }

        async fn mempool_change(
            &self,
            _: Request<Empty>,
        ) -> Result<Response<Self::MempoolChangeStream>, Status> {
            let receiver = self
                .mempool_receiver
                .lock()
                .await
                .take()
                .expect("test only expects one mempool_change subscription");

            Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
        }
    }

    async fn spawn_test_indexer(
        non_finalized_receiver: mpsc::Receiver<Result<BlockAndHash, Status>>,
    ) -> Result<(SocketAddr, JoinHandle<Result<(), tonic::transport::Error>>), BoxError> {
        let (_chain_tip_sender, chain_tip_receiver) = mpsc::channel(1);
        let (_mempool_sender, mempool_receiver) = mpsc::channel(1);

        let indexer = TestIndexer {
            chain_tip_receiver: Arc::new(Mutex::new(Some(chain_tip_receiver))),
            non_finalized_receiver: Arc::new(Mutex::new(Some(non_finalized_receiver))),
            mempool_receiver: Arc::new(Mutex::new(Some(mempool_receiver))),
            chain_tip_subscribed: Arc::new(Mutex::new(None)),
        };

        let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let listen_addr = tcp_listener.local_addr()?;
        let server_task = tokio::spawn(async move {
            Server::builder()
                .add_service(IndexerServer::new(indexer))
                .serve_with_incoming(TcpIncoming::from(tcp_listener))
                .await
        });

        Ok((listen_addr, server_task))
    }

    async fn start_syncer_without_forwarder(
        network: &Network,
        non_finalized_receiver: mpsc::Receiver<Result<BlockAndHash, Status>>,
    ) -> Result<
        (
            tempfile::TempDir,
            tokio::sync::watch::Receiver<NonFinalizedState>,
            JoinHandle<()>,
            JoinHandle<Result<(), tonic::transport::Error>>,
        ),
        BoxError,
    > {
        let (listen_addr, server_task) = spawn_test_indexer(non_finalized_receiver).await?;

        let cache_dir = tempfile::tempdir()?;
        let config = Config {
            cache_dir: cache_dir.path().to_path_buf(),
            ..Config::default()
        };

        let mut primary_state = FinalizedState::new(&config, network);
        let genesis = zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES
            .zcash_deserialize_into::<Arc<Block>>()
            .expect("genesis block test vector should deserialize");
        let genesis = CheckpointVerifiedBlock::from(genesis);
        primary_state
            .commit_finalized_direct(genesis.into(), None, "sync test")
            .expect("genesis block should commit to the primary finalized state");

        let (_read_state, secondary_db, non_finalized_state_sender) =
            zebra_state::init_read_only(config, network);
        secondary_db
            .spawn_try_catch_up_with_primary()
            .await
            .expect("secondary db should catch up to its primary finalized genesis");

        let non_finalized_state_receiver = non_finalized_state_sender.subscribe();
        let indexer_rpc_client = IndexerClient::connect(format!("http://{listen_addr}")).await?;
        let (chain_tip_sender, _latest_chain_tip, _chain_tip_change) =
            ChainTipSender::new(None, network);
        let mut syncer = TrustedChainSync {
            indexer_rpc_client,
            db: secondary_db,
            non_finalized_state: NonFinalizedState::new(network),
            chain_tip_sender,
            non_finalized_state_sender,
        };

        let sync_task = tokio::spawn(async move {
            syncer.sync().await;
        });

        Ok((
            cache_dir,
            non_finalized_state_receiver,
            sync_task,
            server_task,
        ))
    }

    async fn wait_for_non_finalized_tip(
        non_finalized_state_receiver: &mut tokio::sync::watch::Receiver<NonFinalizedState>,
        height: Height,
    ) -> (Height, block::Hash) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if let Some(tip) = non_finalized_state_receiver.borrow().best_tip() {
                    if tip.0 == height {
                        return tip;
                    }
                }

                non_finalized_state_receiver
                    .changed()
                    .await
                    .expect("sync task should keep the non-finalized watch sender alive");
            }
        })
        .await
        .expect("TrustedChainSync should publish the expected non-finalized tip before timeout")
    }

    fn transaction_v4_from_coinbase(coinbase: &Transaction) -> Transaction {
        assert!(
            !coinbase.has_sapling_shielded_data(),
            "conversion assumes sapling shielded data is None"
        );

        Transaction::V4 {
            inputs: coinbase.inputs().to_vec(),
            outputs: coinbase.outputs().to_vec(),
            lock_time: coinbase.lock_time().unwrap_or_else(LockTime::unlocked),
            expiry_height: coinbase.expiry_height().unwrap_or(Height(0)),
            joinsplit_data: None,
            sapling_shielded_data: None,
        }
    }

    fn make_fake_child(parent: &Arc<Block>) -> Arc<Block> {
        let parent_hash = parent.hash();
        let mut child = Block::clone(parent);
        let mut transactions = std::mem::take(&mut child.transactions);
        let mut tx = transactions.remove(0);

        let input = match Arc::make_mut(&mut tx) {
            Transaction::V1 { inputs, .. } => &mut inputs[0],
            Transaction::V2 { inputs, .. } => &mut inputs[0],
            Transaction::V3 { inputs, .. } => &mut inputs[0],
            Transaction::V4 { inputs, .. } => &mut inputs[0],
            Transaction::V5 { inputs, .. } => &mut inputs[0],
            #[cfg(all(zcash_unstable = "nu7", feature = "tx_v6"))]
            Transaction::V6 { inputs, .. } => &mut inputs[0],
        };

        match input {
            transparent::Input::Coinbase { height, .. } => height.0 += 1,
            _ => panic!("block must have a coinbase height to create a child"),
        }

        child.transactions.insert(0, tx);
        Arc::make_mut(&mut child.header).previous_block_hash = parent_hash;

        Arc::new(child)
    }

    fn height_two_child_of_genesis() -> Arc<Block> {
        let genesis = zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES
            .zcash_deserialize_into::<Arc<Block>>()
            .expect("genesis block test vector should deserialize");

        let mut height_two_child = make_fake_child(&make_fake_child(&genesis));
        Arc::make_mut(&mut Arc::make_mut(&mut height_two_child).header).previous_block_hash =
            genesis.hash();
        let coinbase = transaction_v4_from_coinbase(&height_two_child.transactions[0]);
        Arc::make_mut(&mut height_two_child).transactions[0] = Arc::new(coinbase);

        assert_eq!(height_two_child.coinbase_height(), Some(Height(2)));
        assert_eq!(height_two_child.header.previous_block_hash, genesis.hash());

        height_two_child
    }

    #[tokio::test]
    async fn trusted_chain_sync_stream_propagates_transmitted_hash_today() -> Result<(), BoxError> {
        let _init_guard = zebra_test::init();

        let network = Network::Mainnet;
        let (non_finalized_sender, non_finalized_receiver) = mpsc::channel(1);
        let (_cache_dir, mut non_finalized_state_receiver, sync_task, server_task) =
            start_syncer_without_forwarder(&network, non_finalized_receiver).await?;

        let block = height_two_child_of_genesis();
        let actual_hash = block.hash();
        let transmitted_hash = block::Hash([42; 32]);
        assert_ne!(
            transmitted_hash, actual_hash,
            "test hash should differ from the block header hash"
        );

        non_finalized_sender
            .send(Ok(BlockAndHash::new(transmitted_hash, block)))
            .await
            .expect("TrustedChainSync should subscribe to the non-finalized stream");

        let published_tip =
            wait_for_non_finalized_tip(&mut non_finalized_state_receiver, Height(2)).await;

        assert_eq!(
            published_tip,
            (Height(2), transmitted_hash),
            "TrustedChainSync should publish the transmitted hash today"
        );
        assert_ne!(
            published_tip.1, actual_hash,
            "published non-finalized tip should not be recomputed from the block header today"
        );

        sync_task.abort();
        server_task.abort();

        Ok(())
    }

    #[tokio::test]
    async fn trusted_chain_sync_stream_accepts_nonsequential_child_of_genesis_today(
    ) -> Result<(), BoxError> {
        let _init_guard = zebra_test::init();

        let network = Network::Mainnet;
        let (non_finalized_sender, non_finalized_receiver) = mpsc::channel(1);
        let (_cache_dir, mut non_finalized_state_receiver, sync_task, server_task) =
            start_syncer_without_forwarder(&network, non_finalized_receiver).await?;

        let block = height_two_child_of_genesis();
        let hash = block.hash();

        non_finalized_sender
            .send(Ok(BlockAndHash::new(hash, block)))
            .await
            .expect("TrustedChainSync should subscribe to the non-finalized stream");

        let published_tip =
            wait_for_non_finalized_tip(&mut non_finalized_state_receiver, Height(2)).await;

        assert_eq!(
            published_tip,
            (Height(2), hash),
            "TrustedChainSync should publish the non-sequential height-2 child of genesis today"
        );

        sync_task.abort();
        server_task.abort();

        Ok(())
    }

    #[tokio::test]
    async fn trusted_chain_sync_tip_forwarder_exits_on_unfinalized_tip_today(
    ) -> Result<(), BoxError> {
        let _init_guard = zebra_test::init();

        let network = Network::Mainnet;
        let cache_dir = tempfile::tempdir()?;
        let config = Config {
            cache_dir: cache_dir.path().to_path_buf(),
            ..Config::default()
        };
        let _primary_state = FinalizedState::new(&config, &network);
        let (_read_state, secondary_db, _non_finalized_state_sender) =
            zebra_state::init_read_only(config, &network);
        let test_hash = block::Hash([42; 32]);
        secondary_db
            .spawn_try_catch_up_with_primary()
            .await
            .expect("secondary db should be able to catch up with its primary");
        assert!(
            secondary_db.block(test_hash.into()).is_none(),
            "test hash must be absent from finalized storage"
        );

        let (chain_tip_sender, chain_tip_receiver) = mpsc::channel(4);
        let (_non_finalized_sender, non_finalized_receiver) = mpsc::channel(1);
        let (_mempool_sender, mempool_receiver) = mpsc::channel(1);
        let (subscribed_sender, subscribed_receiver) = oneshot::channel();

        let indexer = TestIndexer {
            chain_tip_receiver: Arc::new(Mutex::new(Some(chain_tip_receiver))),
            non_finalized_receiver: Arc::new(Mutex::new(Some(non_finalized_receiver))),
            mempool_receiver: Arc::new(Mutex::new(Some(mempool_receiver))),
            chain_tip_subscribed: Arc::new(Mutex::new(Some(subscribed_sender))),
        };

        let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let listen_addr = tcp_listener.local_addr()?;
        let server_task = tokio::spawn(async move {
            Server::builder()
                .add_service(IndexerServer::new(indexer))
                .serve_with_incoming(TcpIncoming::from(tcp_listener))
                .await
        });

        let indexer_rpc_client = IndexerClient::connect(format!("http://{listen_addr}")).await?;
        let (chain_tip_sender_for_state, _latest_chain_tip, _chain_tip_change) =
            ChainTipSender::new(None, &network);
        let finalized_tip_forwarder = spawn_finalized_tip_forwarder(
            indexer_rpc_client,
            secondary_db,
            chain_tip_sender_for_state.finalized_sender(),
        );

        tokio::time::timeout(std::time::Duration::from_secs(3), subscribed_receiver)
            .await
            .expect("TrustedChainSync should subscribe to chain_tip_change before timeout")
            .expect("test subscription signal should be delivered");

        chain_tip_sender
            .send(Ok(BlockHashAndHeight::new(test_hash, Height(1))))
            .await
            .expect("chain_tip_change stream should still be open before unfinalized tip");

        tokio::time::timeout(std::time::Duration::from_secs(3), finalized_tip_forwarder)
            .await
            .expect("unfinalized best-tip hash should make the forwarding task exit today")
            .expect("forwarding task should not panic");

        server_task.abort();

        Ok(())
    }
}
