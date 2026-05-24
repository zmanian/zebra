//! Fixed test vectors for the mempool.

#![allow(clippy::unwrap_in_result)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use color_eyre::Report;
use tokio::{
    sync::mpsc::error::TryRecvError,
    time::{self, timeout},
};
use tower::{ServiceBuilder, ServiceExt};

use rand::{seq::SliceRandom, thread_rng};
use zebra_chain::{
    amount::Amount,
    block::Block,
    fmt::humantime_seconds,
    parameters::Network,
    serialization::ZcashDeserializeInto,
    transaction::{Hash as TransactionHash, Transaction, UnminedTxId, VerifiedUnminedTx},
    transparent::{self, OutPoint},
};
use zebra_consensus::transaction as tx;
use zebra_state::{Config as StateConfig, CHAIN_TIP_UPDATE_WAIT_LIMIT};
use zebra_test::mock_service::{MockService, PanicAssertion};

use crate::components::{
    mempool::{self, *},
    sync::RecentSyncLengths,
};

/// A [`MockService`] representing the network service.
type MockPeerSet = MockService<zn::Request, zn::Response, PanicAssertion>;

/// The unmocked Zebra state service's type.
type StateService = Buffer<BoxService<zs::Request, zs::Response, zs::BoxError>, zs::Request>;

/// A [`MockService`] representing the Zebra transaction verifier service.
type MockTxVerifier = MockService<tx::Request, tx::Response, PanicAssertion, TransactionError>;

#[derive(Clone, Debug, Eq, PartialEq)]
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
        let labels = key
            .labels()
            .map(|label| (label.key().to_string(), label.value().to_string()))
            .collect();

        self.metrics
            .lock()
            .expect(
                "recorder mutex should not be poisoned because tests do not panic while holding it",
            )
            .push(RecordedMetric {
                name: key.name().to_string(),
                labels,
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

#[tokio::test]
async fn mempool_service_basic() -> Result<(), Report> {
    // Test multiple times to catch intermittent bugs since eviction is randomized
    for _ in 0..10 {
        mempool_service_basic_single().await?;
    }
    Ok(())
}

async fn mempool_service_basic_single() -> Result<(), Report> {
    // Using the mainnet for now
    let network = Network::Mainnet;

    // get the genesis block transactions from the Zcash blockchain.
    let mut unmined_transactions = network.unmined_transactions_in_blocks(1..=10);
    let genesis_transaction = unmined_transactions
        .next()
        .expect("Missing genesis transaction");
    let last_transaction = unmined_transactions.next_back().unwrap();
    let more_transactions = unmined_transactions.collect::<Vec<_>>();

    // Use as cost limit the costs of all transactions that will be
    // inserted except one (the genesis block transaction).
    let cost_limit = more_transactions.iter().map(|tx| tx.cost()).sum();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    // Enable the mempool
    service.enable(&mut recent_syncs).await;

    // Insert the genesis block coinbase transaction into the mempool storage.
    let mut inserted_ids = HashSet::new();
    service
        .storage()
        .insert(genesis_transaction.clone(), Vec::new(), None)?;
    inserted_ids.insert(genesis_transaction.transaction.id);

    // Test `Request::TransactionIds`
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::TransactionIds)
        .await
        .unwrap();
    let genesis_transaction_ids = match response {
        Response::TransactionIds(ids) => ids,
        _ => unreachable!("will never happen in this test"),
    };

    // Test `Request::TransactionsById`
    let genesis_transactions_hash_set = genesis_transaction_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::TransactionsById(
            genesis_transactions_hash_set.clone(),
        ))
        .await
        .unwrap();
    let transactions = match response {
        Response::Transactions(transactions) => transactions,
        _ => unreachable!("will never happen in this test"),
    };

    // Make sure the transaction from the blockchain test vector is the same as the
    // response of `Request::TransactionsById`
    assert_eq!(genesis_transaction.transaction, transactions[0]);

    // Test `Request::TransactionsByMinedId`
    // TODO: use a V5 tx to test if it's really matched by mined ID
    let genesis_transactions_mined_hash_set = genesis_transaction_ids
        .iter()
        .map(|txid| txid.mined_id())
        .collect::<HashSet<_>>();
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::TransactionsByMinedId(
            genesis_transactions_mined_hash_set,
        ))
        .await
        .unwrap();
    let transactions = match response {
        Response::Transactions(transactions) => transactions,
        _ => unreachable!("will never happen in this test"),
    };

    // Make sure the transaction from the blockchain test vector is the same as the
    // response of `Request::TransactionsByMinedId`
    assert_eq!(genesis_transaction.transaction, transactions[0]);

    // Insert more transactions into the mempool storage.
    // This will cause the genesis transaction to be moved into rejected.
    // Skip the last (will be used later)
    for tx in more_transactions {
        inserted_ids.insert(tx.transaction.id);
        // Error must be ignored because a insert can trigger an eviction and
        // an error is returned if the transaction being inserted in chosen.
        let _ = service.storage().insert(tx.clone(), Vec::new(), None);
    }

    // Test `Request::RejectedTransactionIds`
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::RejectedTransactionIds(
            genesis_transactions_hash_set,
        ))
        .await
        .unwrap();
    let rejected_ids = match response {
        Response::RejectedTransactionIds(ids) => ids,
        _ => unreachable!("will never happen in this test"),
    };

    assert!(rejected_ids.is_subset(&inserted_ids));

    // Test `Request::Queue`
    // Use the ID of the last transaction in the list
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![last_transaction.transaction.id.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());
    assert_eq!(service.tx_downloads().in_flight(), 1);

    // Test `Request::QueueStats`
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::QueueStats)
        .await
        .unwrap();

    let (actual_size, actual_bytes, actual_usage) = match response {
        Response::QueueStats {
            size,
            bytes,
            usage,
            fully_notified: None,
        } => (size, bytes, usage),
        _ => unreachable!("expected QueueStats response"),
    };

    // Expected values based on storage contents
    let expected_size = service.storage().transaction_count();
    let expected_bytes: usize = service
        .storage()
        .transactions()
        .values()
        .map(|tx| tx.transaction.size)
        .sum();

    // TODO: Derive memory usage when available
    let expected_usage = expected_bytes;

    assert_eq!(actual_size, expected_size, "QueueStats size mismatch");
    assert_eq!(actual_bytes, expected_bytes, "QueueStats bytes mismatch");
    assert_eq!(actual_usage, expected_usage, "QueueStats usage mismatch");

    Ok(())
}

#[tokio::test]
async fn mempool_queue() -> Result<(), Report> {
    // Test multiple times to catch intermittent bugs since eviction is randomized
    for _ in 0..10 {
        mempool_queue_single().await?;
    }
    Ok(())
}

async fn mempool_queue_single() -> Result<(), Report> {
    // Using the mainnet for now
    let network = Network::Mainnet;

    // Get transactions to use in the test
    let unmined_transactions = network.unmined_transactions_in_blocks(1..=10);
    let mut transactions = unmined_transactions.collect::<Vec<_>>();
    // Split unmined_transactions into:
    // [transactions..., new_tx]
    // A transaction not in the mempool that will be Queued
    let new_tx = transactions.pop().unwrap();

    // Use as cost limit the costs of all transactions that will be
    // inserted except the last.
    let cost_limit = transactions
        .iter()
        .take(transactions.len() - 1)
        .map(|tx| tx.cost())
        .sum();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    // Enable the mempool
    service.enable(&mut recent_syncs).await;

    // Insert [transactions...] into the mempool storage.
    // This will cause the at least one transaction to be rejected, since
    // the cost limit is the sum of all costs except of the last transaction.
    for tx in transactions.iter() {
        // Error must be ignored because a insert can trigger an eviction and
        // an error is returned if the transaction being inserted in chosen.
        let _ = service.storage().insert(tx.clone(), Vec::new(), None);
    }

    // Test `Request::Queue` for a new transaction
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![new_tx.transaction.id.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());

    // Test `Request::Queue` with all previously inserted transactions.
    // They should all be rejected; either because they are already in the mempool,
    // or because they are in the recently evicted list.
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(
            transactions
                .iter()
                .map(|tx| tx.transaction.id.into())
                .collect(),
        ))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), transactions.len());

    // Check if the responses are consistent
    let mut in_mempool_count = 0;
    let mut evicted_count = 0;
    for response in queued_responses {
        match response.unbox_mempool_error() {
            MempoolError::StorageEffectsChain(SameEffectsChainRejectionError::RandomlyEvicted) => {
                evicted_count += 1
            }
            MempoolError::InMempool => in_mempool_count += 1,
            error => panic!("transaction should not be rejected with reason {error:?}"),
        }
    }
    assert_eq!(in_mempool_count, transactions.len() - 1);
    assert_eq!(evicted_count, 1);

    Ok(())
}

#[tokio::test]
async fn mempool_queue_reports_every_gossiped_id_before_download_cap() -> Result<(), Report> {
    let network = Network::Mainnet;
    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    service.enable(&mut recent_syncs).await;

    let requested_count = downloads::MAX_INBOUND_CONCURRENCY + 3;
    let gossiped_txs = (0..requested_count)
        .map(|index| {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&((index + 1) as u64).to_le_bytes());

            UnminedTxId::from_legacy_id(TransactionHash(bytes)).into()
        })
        .collect();

    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(gossiped_txs))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };

    assert_eq!(queued_responses.len(), requested_count);

    let mut accepted_count = 0;
    let mut full_queue_count = 0;

    for response in queued_responses {
        match response {
            Ok(_receiver) => accepted_count += 1,
            Err(error) => match error.unbox_mempool_error() {
                MempoolError::FullQueue => full_queue_count += 1,
                error => panic!("unexpected queue rejection reason: {error:?}"),
            },
        }
    }

    assert_eq!(accepted_count, downloads::MAX_INBOUND_CONCURRENCY);
    assert_eq!(
        full_queue_count,
        requested_count - downloads::MAX_INBOUND_CONCURRENCY
    );
    assert_eq!(
        service.tx_downloads().in_flight(),
        downloads::MAX_INBOUND_CONCURRENCY
    );

    Ok(())
}

#[tokio::test]
async fn mempool_service_disabled() -> Result<(), Report> {
    // Using the mainnet for now
    let network = Network::Mainnet;

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    // get the genesis block transactions from the Zcash blockchain.
    let mut unmined_transactions = network.unmined_transactions_in_blocks(1..=10);
    let genesis_transaction = unmined_transactions
        .next()
        .expect("Missing genesis transaction");
    let more_transactions = unmined_transactions;

    // Test if mempool is disabled (it should start disabled)
    assert!(!service.is_enabled());

    // Enable the mempool
    service.enable(&mut recent_syncs).await;

    assert!(service.is_enabled());

    // Insert the genesis block coinbase transaction into the mempool storage.
    service
        .storage()
        .insert(genesis_transaction.clone(), Vec::new(), None)?;

    // Test if the mempool answers correctly (i.e. is enabled)
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::TransactionIds)
        .await
        .unwrap();
    let _genesis_transaction_ids = match response {
        Response::TransactionIds(ids) => ids,
        _ => unreachable!("will never happen in this test"),
    };

    // Queue a transaction for download
    // Use the ID of the last transaction in the list
    let txid = more_transactions.last().unwrap().transaction.id;
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![txid.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());
    assert_eq!(service.tx_downloads().in_flight(), 1);

    // Disable the mempool
    service.disable(&mut recent_syncs).await;

    // Test if mempool is disabled again
    assert!(!service.is_enabled());

    // Test if the mempool returns no transactions when disabled
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::TransactionIds)
        .await
        .unwrap();
    match response {
        Response::TransactionIds(ids) => {
            assert_eq!(
                ids.len(),
                0,
                "mempool should return no transactions when disabled"
            )
        }
        _ => unreachable!("will never happen in this test"),
    };

    // Test if the mempool returns to Queue requests correctly when disabled
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![txid.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };

    assert_eq!(queued_responses.len(), 1);
    assert_eq!(
        queued_responses
            .into_iter()
            .next()
            .unwrap()
            .unbox_mempool_error(),
        MempoolError::Disabled
    );

    // Test if mempool returns to QueueStats request correctly when disabled
    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::QueueStats)
        .await
        .unwrap();

    let (size, bytes, usage, fully_notified) = match response {
        Response::QueueStats {
            size,
            bytes,
            usage,
            fully_notified,
        } => (size, bytes, usage, fully_notified),
        _ => unreachable!("expected QueueStats response"),
    };

    assert_eq!(size, 0, "size should be zero when mempool is disabled");
    assert_eq!(bytes, 0, "bytes should be zero when mempool is disabled");
    assert_eq!(usage, 0, "usage should be zero when mempool is disabled");
    assert_eq!(
        fully_notified, None,
        "fully_notified should be None when mempool is disabled"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn mempool_cancel_mined() -> Result<(), Report> {
    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES
        .zcash_deserialize_into()
        .unwrap();
    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES
        .zcash_deserialize_into()
        .unwrap();

    // Using the mainnet for now
    let network = Network::Mainnet;

    let (
        mut mempool,
        _peer_set,
        mut state_service,
        mut chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        mut mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    // Enable the mempool
    mempool.enable(&mut recent_syncs).await;
    assert!(mempool.is_enabled());

    // Query the mempool to make it poll chain_tip_change
    mempool.dummy_call().await;

    // Push block 1 to the state
    state_service
        .ready()
        .await
        .unwrap()
        .call(zebra_state::Request::CommitCheckpointVerifiedBlock(
            block1.clone().into(),
        ))
        .await
        .unwrap();

    // Wait for the chain tip update
    if let Err(timeout_error) = timeout(
        CHAIN_TIP_UPDATE_WAIT_LIMIT,
        chain_tip_change.wait_for_tip_change(),
    )
    .await
    .map(|change_result| change_result.expect("unexpected chain tip update failure"))
    {
        info!(
            timeout = ?humantime_seconds(CHAIN_TIP_UPDATE_WAIT_LIMIT),
            ?timeout_error,
            "timeout waiting for chain tip change after committing block"
        );
    }

    // Query the mempool to make it poll chain_tip_change
    mempool.dummy_call().await;

    // Queue transaction from block 2 for download.
    // It can't be queued before because block 1 triggers a network upgrade,
    // which cancels all downloads.
    let txid = block2.transactions[0].unmined_id();
    let response = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![txid.into()]))
        .await
        .unwrap();
    let mut queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);

    let queued_response = queued_responses
        .pop()
        .expect("already checked that there is exactly 1 item in Vec")
        .expect("initial queue checks result should be Ok");

    assert_eq!(mempool.tx_downloads().in_flight(), 1);

    // Push block 2 to the state
    state_service
        .oneshot(zebra_state::Request::CommitCheckpointVerifiedBlock(
            block2.clone().into(),
        ))
        .await
        .unwrap();

    // Wait for the chain tip update
    if let Err(timeout_error) = timeout(
        CHAIN_TIP_UPDATE_WAIT_LIMIT,
        chain_tip_change.wait_for_tip_change(),
    )
    .await
    .map(|change_result| change_result.expect("unexpected chain tip update failure"))
    {
        info!(
            timeout = ?humantime_seconds(CHAIN_TIP_UPDATE_WAIT_LIMIT),
            ?timeout_error,
            "timeout waiting for chain tip change after committing block"
        );
    }

    // This is done twice because after the first query the cancellation
    // is picked up by select!, and after the second the mempool gets the
    // result and the download future is removed.
    for _ in 0..2 {
        // Query the mempool just to poll it and make it cancel the download.
        mempool.dummy_call().await;
        // Sleep to avoid starvation and make sure the cancellation is picked up.
        time::sleep(time::Duration::from_millis(100)).await;
    }

    // Check if download was cancelled.
    assert_eq!(mempool.tx_downloads().in_flight(), 0);

    assert!(
        queued_response
            .await
            .expect("channel should not be closed")
            .is_err(),
        "queued tx should fail to download and verify due to chain tip change"
    );

    let mempool_change = timeout(Duration::from_secs(3), mempool_transaction_receiver.recv())
        .await
        .expect("should not timeout")
        .expect("recv should return Ok");

    assert_eq!(
        mempool_change,
        MempoolChange::invalidated([txid].into_iter().collect())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn mempool_cancel_downloads_after_network_upgrade() -> Result<(), Report> {
    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES
        .zcash_deserialize_into()
        .unwrap();
    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES
        .zcash_deserialize_into()
        .unwrap();

    // Using the mainnet for now
    let network = Network::Mainnet;

    let (
        mut mempool,
        mut peer_set,
        mut state_service,
        mut chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    // Enable the mempool
    mempool.enable(&mut recent_syncs).await;
    assert!(mempool.is_enabled());

    // Queue transaction from block 2 for download
    let txid = block2.transactions[0].unmined_id();
    let response = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![txid.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());
    assert_eq!(mempool.tx_downloads().in_flight(), 1);

    // Query the mempool to make it poll chain_tip_change
    mempool.dummy_call().await;

    // Push block 1 to the state. This is considered a network upgrade,
    // and thus must cancel all pending transaction downloads.
    state_service
        .ready()
        .await
        .unwrap()
        .call(zebra_state::Request::CommitCheckpointVerifiedBlock(
            block1.clone().into(),
        ))
        .await
        .unwrap();

    // Wait for the chain tip update
    if let Err(timeout_error) = timeout(
        CHAIN_TIP_UPDATE_WAIT_LIMIT,
        chain_tip_change.wait_for_tip_change(),
    )
    .await
    .map(|change_result| change_result.expect("unexpected chain tip update failure"))
    {
        info!(
            timeout = ?humantime_seconds(CHAIN_TIP_UPDATE_WAIT_LIMIT),
            ?timeout_error,
            "timeout waiting for chain tip change after committing block"
        );
    }

    // Ignore all the previous network requests.
    while let Some(_request) = peer_set.try_next_request().await {}

    // Query the mempool to make it poll chain_tip_change
    mempool.dummy_call().await;

    // Check if download was cancelled and transaction was retried.
    let request = peer_set
        .try_next_request()
        .await
        .expect("unexpected missing mempool retry");

    assert_eq!(
        request.request(),
        &zebra_network::Request::TransactionsById(iter::once(txid).collect()),
    );
    assert_eq!(mempool.tx_downloads().in_flight(), 1);

    Ok(())
}

/// Check if a transaction that fails verification is rejected by the mempool.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_failed_verification_is_rejected() -> Result<(), Report> {
    // Using the mainnet for now
    let network = Network::Mainnet;

    let (
        mut mempool,
        _peer_set,
        _state_service,
        _chain_tip_change,
        mut tx_verifier,
        mut recent_syncs,
        mut mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    // Get transactions to use in the test
    let mut unmined_transactions = network.unmined_transactions_in_blocks(1..=2);
    let rejected_tx = unmined_transactions.next().unwrap().clone();

    // Enable the mempool
    mempool.enable(&mut recent_syncs).await;

    // Queue first transaction for verification
    // (queue the transaction itself to avoid a download).
    let request = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![rejected_tx.transaction.clone().into()]));
    // Make the mock verifier return that the transaction is invalid.
    let verification = tx_verifier.expect_request_that(|_| true).map(|responder| {
        responder.respond(Err(TransactionError::BadBalance));
    });
    let (response, _) = futures::join!(request, verification);
    let queued_responses = match response.unwrap() {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    // Check that the request was enqueued successfully.
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());

    for _ in 0..2 {
        // Query the mempool just to poll it and make get the downloader/verifier result.
        mempool.dummy_call().await;
        // Sleep to avoid starvation and make sure the verification failure is picked up.
        time::sleep(time::Duration::from_millis(100)).await;
    }

    // Try to queue the same transaction by its ID and check if it's correctly
    // rejected.
    let response = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![rejected_tx.transaction.id.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(matches!(
        queued_responses
            .into_iter()
            .next()
            .unwrap()
            .unbox_mempool_error(),
        MempoolError::StorageExactTip(ExactTipRejectionError::FailedVerification(_))
    ));

    let mempool_change = timeout(Duration::from_secs(3), mempool_transaction_receiver.recv())
        .await
        .expect("should not timeout")
        .expect("recv should return Ok");

    assert_eq!(
        mempool_change,
        MempoolChange::invalidated([rejected_tx.transaction.id].into_iter().collect())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn full_misbehavior_channel_drops_score_bearing_mempool_report_today() -> Result<(), Report> {
    let network = Network::Mainnet;

    let (
        mut mempool,
        mut peer_set,
        _state_service,
        _chain_tip_change,
        mut tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;
    let (misbehavior_tx, mut misbehavior_rx) = tokio::sync::mpsc::channel(1);

    let sentinel_addr = zn::PeerSocketAddr::from(([127, 0, 0, 1], 8233));
    misbehavior_tx
        .try_send((sentinel_addr, 1))
        .expect("channel should accept the sentinel");
    mempool.misbehavior_sender = misbehavior_tx;

    let rejected_tx = network
        .unmined_transactions_in_blocks(1..=2)
        .next()
        .expect("test network should have at least one unmined transaction");
    let advertiser_addr = zn::PeerSocketAddr::from(([127, 0, 0, 2], 8233));
    let verifier_error = TransactionError::BadBalance;
    assert_eq!(
        verifier_error.mempool_misbehavior_score(),
        100,
        "test error should carry a mempool misbehavior score"
    );

    mempool.enable(&mut recent_syncs).await;

    let request = mempool
        .ready()
        .await
        .expect("mempool should become ready")
        .call(Request::Queue(vec![rejected_tx.transaction.id.into()]));
    let download = peer_set
        .expect_request_that(|request| matches!(request, zn::Request::TransactionsById(_)))
        .map(|responder| {
            responder.respond(zn::Response::Transactions(vec![
                zn::InventoryResponse::Available((
                    rejected_tx.transaction.clone(),
                    Some(advertiser_addr),
                )),
            ]));
        });
    let (response, _) = futures::join!(request, download);
    let Response::Queued(queue_responses) = response.expect("queue request should succeed") else {
        panic!("wrong response from mempool to Queue request");
    };
    assert_eq!(queue_responses.len(), 1);
    assert!(queue_responses[0].is_ok());

    tx_verifier
        .expect_request_that(|_| true)
        .map(|responder| {
            responder.respond(Err(verifier_error));
        })
        .await;

    for _ in 0..2 {
        mempool.dummy_call().await;
        time::sleep(time::Duration::from_millis(100)).await;
    }

    assert_eq!(misbehavior_rx.try_recv(), Ok((sentinel_addr, 1)));
    assert_eq!(misbehavior_rx.try_recv(), Err(TryRecvError::Empty));

    Ok(())
}

/// Check that mempool verification failures use the raw transaction error string
/// as the Prometheus `reason` label.
#[test]
fn mempool_failed_verify_metric_reason_uses_raw_transaction_error_today() -> Result<(), Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime should build for this metrics capture test");
    let recorder = RecordingRecorder::default();

    let reasons = metrics::with_local_recorder(&recorder, || {
        runtime.block_on(async {
            let network = Network::Mainnet;

            let (
                mut mempool,
                _peer_set,
                _state_service,
                _chain_tip_change,
                mut tx_verifier,
                mut recent_syncs,
                _mempool_transaction_receiver,
            ) = setup(&network, u64::MAX, true).await;

            let mut unmined_transactions = network.unmined_transactions_in_blocks(1..=3);
            let rejected_txs = [
                unmined_transactions
                    .next()
                    .expect("test network should have a first unmined transaction"),
                unmined_transactions
                    .next()
                    .expect("test network should have a second unmined transaction"),
            ];
            let errors = [
                TransactionError::DuplicateTransparentSpend(transparent::OutPoint::from_usize(
                    rejected_txs[0].transaction.id.mined_id(),
                    0,
                )),
                TransactionError::DuplicateTransparentSpend(transparent::OutPoint::from_usize(
                    rejected_txs[1].transaction.id.mined_id(),
                    1,
                )),
            ];
            let reasons = errors
                .iter()
                .map(|error| {
                    mempool::downloads::TransactionDownloadVerifyError::Invalid {
                        error: error.clone(),
                        advertiser_addr: None,
                    }
                    .to_string()
                })
                .collect::<Vec<_>>();

            mempool.enable(&mut recent_syncs).await;

            for (rejected_tx, error) in rejected_txs.into_iter().zip(errors) {
                let request = mempool
                    .ready()
                    .await
                    .expect("mempool should become ready")
                    .call(Request::Queue(vec![rejected_tx.transaction.into()]));
                let verification = tx_verifier.expect_request_that(|_| true).map(|responder| {
                    responder.respond(Err(error));
                });
                let (response, _) = futures::join!(request, verification);
                let Response::Queued(queue_responses) =
                    response.expect("queue request should succeed")
                else {
                    panic!("wrong response from mempool to Queue request");
                };

                assert_eq!(queue_responses.len(), 1);
                assert!(queue_responses[0].is_ok());
            }

            for _ in 0..2 {
                mempool.dummy_call().await;
                time::sleep(time::Duration::from_millis(100)).await;
            }

            reasons
        })
    });

    let metrics = recorder.recorded_metrics();

    for reason in reasons {
        assert!(
            metric_has_label(
                &metrics,
                "mempool.failed.verify.tasks.total",
                "reason",
                &reason
            ),
            "mempool failure metric should use the raw transaction error as a Prometheus label: {metrics:?}"
        );
    }

    Ok(())
}

/// Check that an internal verifier failure is exact-tip rejected today.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_internal_verifier_error_is_exact_tip_rejected_today() -> Result<(), Report> {
    let network = Network::Mainnet;

    let (
        mut mempool,
        _peer_set,
        _state_service,
        _chain_tip_change,
        mut tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    let rejected_tx = network
        .unmined_transactions_in_blocks(1..=2)
        .next()
        .expect("test network should have at least one unmined transaction");
    let txid = rejected_tx.transaction.id;

    mempool.enable(&mut recent_syncs).await;

    let request = mempool
        .ready()
        .await
        .expect("mempool should become ready")
        .call(Request::Queue(vec![rejected_tx.transaction.into()]));
    let verification = tx_verifier.expect_request_that(|_| true).map(|responder| {
        responder.respond(Err(TransactionError::InternalDowncastError(
            "synthetic verifier infrastructure failure".to_string(),
        )));
    });
    let (response, _) = futures::join!(request, verification);
    let Response::Queued(queue_responses) = response.expect("queue request should succeed") else {
        panic!("wrong response from mempool to Queue request");
    };
    assert_eq!(queue_responses.len(), 1);
    assert!(queue_responses[0].is_ok());

    for _ in 0..2 {
        mempool.dummy_call().await;
        time::sleep(time::Duration::from_millis(100)).await;
    }

    let response = mempool
        .ready()
        .await
        .expect("mempool should become ready")
        .call(Request::Queue(vec![txid.into()]))
        .await
        .expect("queue request should succeed");
    let Response::Queued(mut queue_responses) = response else {
        panic!("wrong response from mempool to Queue request");
    };
    assert_eq!(queue_responses.len(), 1);

    assert!(matches!(
        queue_responses.remove(0).unbox_mempool_error(),
        MempoolError::StorageExactTip(ExactTipRejectionError::FailedVerification(
            TransactionError::InternalDowncastError(_)
        ))
    ));

    Ok(())
}

/// Check if a transaction that fails download is _not_ rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_failed_download_is_not_rejected() -> Result<(), Report> {
    // Using the mainnet for now
    let network = Network::Mainnet;

    let (
        mut mempool,
        mut peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        mut mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    // Get transactions to use in the test
    let mut unmined_transactions = network.unmined_transactions_in_blocks(1..=2);
    let rejected_valid_tx = unmined_transactions.next().unwrap().clone();

    // Enable the mempool
    mempool.enable(&mut recent_syncs).await;

    // Queue second transaction for download and verification.
    let request = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![rejected_valid_tx
            .transaction
            .id
            .into()]));
    // Make the mock peer set return that the download failed.
    let verification = peer_set
        .expect_request_that(|r| matches!(r, zn::Request::TransactionsById(_)))
        .map(|responder| {
            responder.respond(zn::Response::Transactions(vec![]));
        });
    let (response, _) = futures::join!(request, verification);
    let queued_responses = match response.unwrap() {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    // Check that the request was enqueued successfully.
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());

    for _ in 0..2 {
        // Query the mempool just to poll it and make get the downloader/verifier result.
        mempool.dummy_call().await;
        // Sleep to avoid starvation and make sure the download failure is picked up.
        time::sleep(time::Duration::from_millis(100)).await;
    }

    // Try to queue the same transaction by its ID and check if it's not being
    // rejected.
    let response = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![rejected_valid_tx
            .transaction
            .id
            .into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());

    let mempool_change = timeout(Duration::from_secs(3), mempool_transaction_receiver.recv())
        .await
        .expect("should not timeout")
        .expect("recv should return Ok");

    assert_eq!(
        mempool_change,
        MempoolChange::invalidated([rejected_valid_tx.transaction.id].into_iter().collect())
    );

    Ok(())
}

/// Check that transactions are re-verified if the tip changes
/// during verification.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reverifies_after_tip_change() -> Result<(), Report> {
    let network = Network::Mainnet;

    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES
        .zcash_deserialize_into()
        .unwrap();
    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES
        .zcash_deserialize_into()
        .unwrap();
    let block3: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_3_BYTES
        .zcash_deserialize_into()
        .unwrap();

    let (
        mut mempool,
        mut peer_set,
        mut state_service,
        mut chain_tip_change,
        mut tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    // Enable the mempool
    mempool.enable(&mut recent_syncs).await;
    assert!(mempool.is_enabled());

    // Queue transaction from block 3 for download
    let tx = block3.transactions[0].clone();
    let txid = block3.transactions[0].unmined_id();
    let response = mempool
        .ready()
        .await
        .unwrap()
        .call(Request::Queue(vec![txid.into()]))
        .await
        .unwrap();
    let queued_responses = match response {
        Response::Queued(queue_responses) => queue_responses,
        _ => unreachable!("will never happen in this test"),
    };
    assert_eq!(queued_responses.len(), 1);
    assert!(queued_responses[0].is_ok());
    assert_eq!(mempool.tx_downloads().in_flight(), 1);

    // Verify the transaction

    peer_set
        .expect_request_that(|req| matches!(req, zn::Request::TransactionsById(_)))
        .map(|responder| {
            responder.respond(zn::Response::Transactions(vec![
                zn::InventoryResponse::Available((tx.clone().into(), None)),
            ]));
        })
        .await;

    tx_verifier
        .expect_request_that(|_| true)
        .map(|responder| {
            let transaction = responder
                .request()
                .clone()
                .mempool_transaction()
                .expect("unexpected non-mempool request");

            // Set a dummy fee and sigops.
            responder.respond(transaction::Response::from(
                VerifiedUnminedTx::new(
                    transaction,
                    Amount::try_from(1_000_000).expect("invalid value"),
                    0,
                    0,
                    std::sync::Arc::new(vec![]),
                )
                .expect("verification should pass"),
            ));
        })
        .await;

    // Push block 1 to the state. This is considered a network upgrade,
    // and must cancel all pending transaction downloads with a `TipAction::Reset`.
    state_service
        .ready()
        .await
        .unwrap()
        .call(zebra_state::Request::CommitCheckpointVerifiedBlock(
            block1.clone().into(),
        ))
        .await
        .unwrap();

    // Wait for the chain tip update without a timeout
    // (skipping the chain tip change here will fail the test)
    chain_tip_change
        .wait_for_tip_change()
        .await
        .expect("unexpected chain tip update failure");

    // Query the mempool to make it poll chain_tip_change and try reverifying its state for the `TipAction::Reset`
    mempool.dummy_call().await;

    // Check that there is still an in-flight tx_download and that
    // no transactions were inserted in the mempool.
    assert_eq!(mempool.tx_downloads().in_flight(), 1);
    assert_eq!(mempool.storage().transaction_count(), 0);

    // Verify the transaction again

    peer_set
        .expect_request_that(|req| matches!(req, zn::Request::TransactionsById(_)))
        .map(|responder| {
            responder.respond(zn::Response::Transactions(vec![
                zn::InventoryResponse::Available((tx.into(), None)),
            ]));
        })
        .await;

    // Verify the transaction now that the mempool has already checked chain_tip_change
    tx_verifier
        .expect_request_that(|_| true)
        .map(|responder| {
            let transaction = responder
                .request()
                .clone()
                .mempool_transaction()
                .expect("unexpected non-mempool request");

            // Set a dummy fee and sigops.
            responder.respond(transaction::Response::from(
                VerifiedUnminedTx::new(
                    transaction,
                    Amount::try_from(1_000_000).expect("invalid value"),
                    0,
                    0,
                    std::sync::Arc::new(vec![]),
                )
                .expect("verification should pass"),
            ));
        })
        .await;

    // Push block 2 to the state. This will increase the tip height past the expected
    // tip height that the tx was verified at.
    state_service
        .ready()
        .await
        .unwrap()
        .call(zebra_state::Request::CommitCheckpointVerifiedBlock(
            block2.clone().into(),
        ))
        .await
        .unwrap();

    // Wait for the chain tip update without a timeout
    // (skipping the chain tip change here will fail the test)
    chain_tip_change
        .wait_for_tip_change()
        .await
        .expect("unexpected chain tip update failure");

    // Query the mempool to make it poll tx_downloads.pending and try reverifying transactions
    // because the tip height has changed.
    mempool.dummy_call().await;

    // Check that there is still an in-flight tx_download and that
    // no transactions were inserted in the mempool.
    assert_eq!(mempool.tx_downloads().in_flight(), 1);
    assert_eq!(mempool.storage().transaction_count(), 0);

    Ok(())
}

/// Checks that the mempool service responds to AwaitOutput requests after verifying transactions
/// that create those outputs, or immediately if the outputs had been created by transaction that
/// are already in the mempool.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_responds_to_await_output() -> Result<(), Report> {
    let network = Network::Mainnet;

    let (
        mut mempool,
        _peer_set,
        _state_service,
        _chain_tip_change,
        mut tx_verifier,
        mut recent_syncs,
        mut mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;
    mempool.enable(&mut recent_syncs).await;

    let verified_unmined_tx = network
        .unmined_transactions_in_blocks(1..=10)
        .find(|tx| !tx.transaction.transaction.outputs().is_empty())
        .expect("should have at least 1 tx with transparent outputs");

    let unmined_tx = verified_unmined_tx.transaction.clone();
    let unmined_tx_id = unmined_tx.id;
    let output_index = 0;
    let outpoint = OutPoint::from_usize(unmined_tx.id.mined_id(), output_index);
    let expected_output = unmined_tx
        .transaction
        .outputs()
        .get(output_index)
        .expect("already checked that tx has outputs")
        .clone();

    // Call mempool with an AwaitOutput request

    let request = Request::AwaitOutput(outpoint);
    let await_output_response_fut = mempool.ready().await.unwrap().call(request);

    // Queue the transaction with the pending output to be added to the mempool

    let request = Request::Queue(vec![Gossip::Tx(unmined_tx)]);
    let queue_response_fut = mempool.ready().await.unwrap().call(request);
    let mock_verify_tx_fut = tx_verifier.expect_request_that(|_| true).map(|responder| {
        responder.respond(transaction::Response::Mempool {
            transaction: verified_unmined_tx,
            spent_mempool_outpoints: Vec::new(),
        });
    });

    let (response, _) = futures::join!(queue_response_fut, mock_verify_tx_fut);
    let Response::Queued(mut results) = response.expect("response should be Ok") else {
        panic!("wrong response from mempool to Queued request");
    };

    let result_rx = results.remove(0).expect("should pass initial checks");
    assert!(results.is_empty(), "should have 1 result for 1 queued tx");

    // Wait for post-verification steps in mempool's Downloads
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Note: Buffered services shouldn't be polled without being called.
    //       See `mempool::Request::CheckForVerifiedTransactions` for more details.
    mempool
        .ready()
        .await
        .expect("polling mempool should succeed");

    tokio::time::timeout(Duration::from_secs(10), result_rx)
        .await
        .expect("should not time out")
        .expect("mempool tx verification result channel should not be closed")
        .expect("mocked verification should be successful");

    assert_eq!(
        mempool.storage().transaction_count(),
        1,
        "should have 1 transaction in mempool's verified set"
    );

    assert_eq!(
        mempool.storage().created_output(&outpoint),
        Some(expected_output.clone()),
        "created output should match expected output"
    );

    // Check that the AwaitOutput request has been responded to after the relevant tx was added to the verified set

    let response_fut = tokio::time::timeout(Duration::from_secs(30), await_output_response_fut);
    let response = response_fut
        .await
        .expect("should not time out")
        .expect("should not return RecvError");

    let Response::UnspentOutput(response) = response else {
        panic!("wrong response from mempool to AwaitOutput request");
    };

    assert_eq!(
        response, expected_output,
        "AwaitOutput response should match expected output"
    );

    // Check that the mempool responds to AwaitOutput requests correctly when the outpoint is already in its `created_outputs` collection too.

    let request = Request::AwaitOutput(outpoint);
    let await_output_response_fut = mempool.ready().await.unwrap().call(request);
    let response_fut = tokio::time::timeout(Duration::from_secs(30), await_output_response_fut);
    let response = response_fut
        .await
        .expect("should not time out")
        .expect("should not return RecvError");

    let Response::UnspentOutput(response) = response else {
        panic!("wrong response from mempool to AwaitOutput request");
    };

    assert_eq!(
        response, expected_output,
        "AwaitOutput response should match expected output"
    );

    let mempool_change = timeout(Duration::from_secs(3), mempool_transaction_receiver.recv())
        .await
        .expect("should not timeout")
        .expect("recv should return Ok");

    assert_eq!(
        mempool_change,
        MempoolChange::added([unmined_tx_id].into_iter().collect())
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn mempool_dropped_await_output_waiter_survives_poll_until_pruned_today() -> Result<(), Report>
{
    let network = Network::Mainnet;

    let (
        mut mempool,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;
    mempool.enable(&mut recent_syncs).await;

    let missing_outpoint = OutPoint::from_usize(TransactionHash([9; 32]), 0);

    let await_output_fut = mempool
        .ready()
        .await
        .expect("mempool should become ready")
        .call(Request::AwaitOutput(missing_outpoint));
    assert_eq!(mempool.storage().pending_outputs.len(), 1);

    drop(await_output_fut);

    mempool.dummy_call().await;
    assert_eq!(
        mempool.storage().pending_outputs.len(),
        1,
        "ordinary mempool polling does not prune abandoned AwaitOutput waiters today"
    );

    mempool.storage().pending_outputs.prune();
    assert_eq!(mempool.storage().pending_outputs.len(), 0);

    Ok(())
}

#[tokio::test(start_paused = true)]
async fn mempool_timeout_retains_pushed_transaction_request_today() -> Result<(), Report> {
    let network = Network::Mainnet;

    let (
        mut mempool,
        _peer_set,
        _state_service,
        _chain_tip_change,
        mut tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, u64::MAX, true).await;

    mempool.enable(&mut recent_syncs).await;
    assert!(mempool.is_enabled());

    let unmined_tx = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("test network should have at least one unmined transaction")
        .transaction;
    let txid = unmined_tx.id;

    let response = mempool
        .ready()
        .await
        .expect("mempool should become ready")
        .call(Request::Queue(vec![Gossip::Tx(unmined_tx)]))
        .await
        .expect("queue request should succeed");
    let Response::Queued(mut queue_results) = response else {
        panic!("wrong response from mempool to Queue request");
    };
    let _result_rx = queue_results
        .remove(0)
        .expect("initial direct transaction queue should pass today");
    assert!(queue_results.is_empty(), "should have one result");
    assert_eq!(mempool.tx_downloads().in_flight(), 1);

    let _pending_verifier_request = tx_verifier.expect_request_that(|_| true).await;
    tokio::time::advance(mempool::crawler::RATE_LIMIT_DELAY + Duration::from_secs(1)).await;
    tokio::task::yield_now().await;

    mempool.dummy_call().await;
    assert_eq!(mempool.tx_downloads().in_flight(), 0);

    let retained_request = mempool
        .tx_downloads()
        .transaction_requests()
        .find(|request| request.id() == txid)
        .expect("timeout path should retain pushed transaction request today");

    assert!(
        retained_request.tx().is_some(),
        "mempool timeout should retain full pushed transaction contents today"
    );

    let response = mempool
        .ready()
        .await
        .expect("mempool should become ready")
        .call(Request::Queue(vec![txid.into()]))
        .await
        .expect("queue request should succeed");
    let Response::Queued(mut queue_results) = response else {
        panic!("wrong response from mempool to Queue request");
    };
    assert_eq!(
        queue_results.remove(0).unbox_mempool_error(),
        MempoolError::AlreadyQueued,
    );
    assert!(queue_results.is_empty(), "should have one result");

    Ok(())
}

/// Check that verified transactions are rejected if non-standard
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_non_standard() -> Result<(), Report> {
    let network = Network::Mainnet;

    // pick a random transaction from the dummy Zcash blockchain
    let unmined_transactions = network.unmined_transactions_in_blocks(1..=10);
    let transactions = unmined_transactions.collect::<Vec<_>>();
    let mut rng = thread_rng();
    let mut last_transaction = transactions
        .choose(&mut rng)
        .expect("Missing transaction")
        .clone();

    last_transaction.height = Some(Height(100_000));

    // Modify the transaction to make it non-standard.
    // This is done by replacing its outputs with a dust output.
    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![transparent::Output {
        value: Amount::new(10), // this is below the dust threshold
        lock_script: p2pkh_script([0u8; 20]),
    }];
    last_transaction.transaction.transaction = tx;

    // Set cost limit to the cost of the transaction we will try to insert.
    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    // Enable the mempool
    service.enable(&mut recent_syncs).await;

    // Insert the modified transaction into the mempool storage.
    // Expect insertion to fail for non-standard transaction.
    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(storage::NonStandardTransactionError::IsDust)
    );

    Ok(())
}

/// Check that standard OP_RETURN outputs are accepted when datacarrier is enabled.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_accept_standard_op_return() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![transparent::Output {
        value: Amount::new(0),
        lock_script: op_return_script(&[0x01]),
    }];
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)?;

    Ok(())
}

/// Check that oversized OP_RETURN scripts are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_op_return_too_large() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![transparent::Output {
        value: Amount::new(0),
        lock_script: op_return_script(&[0x03]),
    }];
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();
    // Shrink the OP_RETURN size limit to trigger the oversized rejection path.
    let mempool_config = mempool::Config {
        tx_cost_limit: cost_limit,
        max_datacarrier_bytes: Some(2),
        ..Default::default()
    };

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup_with_mempool_config(&network, mempool_config, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(
            storage::NonStandardTransactionError::DataCarrierTooLarge
        )
    );

    Ok(())
}

/// Check that multiple OP_RETURN outputs are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_multi_op_return() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![
        transparent::Output {
            value: Amount::new(0),
            lock_script: op_return_script(&[0x04]),
        },
        transparent::Output {
            value: Amount::new(0),
            lock_script: op_return_script(&[0x05]),
        },
    ];
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(storage::NonStandardTransactionError::MultiOpReturn)
    );

    Ok(())
}

/// Check that non-standard scriptPubKeys are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_non_standard_scriptpubkey() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![transparent::Output {
        value: Amount::new(1000),
        lock_script: transparent::Script::new(&[0x00]),
    }];
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(
            storage::NonStandardTransactionError::ScriptPubKeyNonStandard
        )
    );

    Ok(())
}

/// Check that bare multisig outputs are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_bare_multisig() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![transparent::Output {
        value: Amount::new(1000),
        lock_script: multisig_script(1, 1),
    }];
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(storage::NonStandardTransactionError::BareMultiSig)
    );

    Ok(())
}

/// Check that oversized bare multisig outputs are rejected as non-standard.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_large_multisig() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    *tx_mut.outputs_mut() = vec![transparent::Output {
        value: Amount::new(1000),
        lock_script: multisig_script(1, 4),
    }];
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(
            storage::NonStandardTransactionError::ScriptPubKeyNonStandard
        )
    );

    Ok(())
}

/// Check that oversized scriptSig inputs are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_large_scriptsig() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = pick_transaction_with_prevout(&network);

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    set_first_prevout_unlock_script(tx_mut, transparent::Script::new(&vec![0u8; 1651]));
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(
            storage::NonStandardTransactionError::ScriptSigTooLarge
        )
    );

    Ok(())
}

/// Check that non-push-only scriptSig inputs are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_non_push_only_scriptsig() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = pick_transaction_with_prevout(&network);

    last_transaction.height = Some(Height(100_000));

    let mut tx = last_transaction.transaction.transaction.clone();
    let tx_mut = Arc::make_mut(&mut tx);
    set_first_prevout_unlock_script(tx_mut, transparent::Script::new(&[0xac]));
    last_transaction.transaction.transaction = tx;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard tx");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(
            storage::NonStandardTransactionError::ScriptSigNotPushOnly
        )
    );

    Ok(())
}

/// Check that transactions with too many sigops are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_too_many_sigops() -> Result<(), Report> {
    let network = Network::Mainnet;

    let mut last_transaction = network
        .unmined_transactions_in_blocks(1..=10)
        .next()
        .expect("missing transaction");

    last_transaction.height = Some(Height(100_000));

    // Set the legacy sigop count above the MAX_STANDARD_TX_SIGOPS limit of 4000.
    last_transaction.legacy_sigop_count = 4001;

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for too many sigops");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(storage::NonStandardTransactionError::TooManySigops)
    );

    Ok(())
}

/// Check that transactions with non-standard inputs (non-standard spent output script)
/// are rejected.
#[tokio::test(flavor = "multi_thread")]
async fn mempool_reject_non_standard_inputs() -> Result<(), Report> {
    let network = Network::Mainnet;

    // Use a transaction that has at least one transparent PrevOut input.
    let mut last_transaction = pick_transaction_with_prevout(&network);

    last_transaction.height = Some(Height(100_000));

    // Provide a non-standard spent output script so are_inputs_standard() returns false.
    // Use a script that doesn't match any known template (OP_1 OP_2 OP_ADD).
    let non_standard_script = transparent::Script::new(&[0x51, 0x52, 0x93]);
    let non_standard_output = transparent::Output {
        value: 0u64.try_into().unwrap(),
        lock_script: non_standard_script,
    };
    // Provide one spent output per transparent input (including coinbase inputs in the count,
    // since are_inputs_standard expects spent_outputs.len() == tx.inputs().len()).
    let input_count = last_transaction.transaction.transaction.inputs().len();
    last_transaction.spent_outputs = std::sync::Arc::new(vec![non_standard_output; input_count]);

    let cost_limit = last_transaction.cost();

    let (
        mut service,
        _peer_set,
        _state_service,
        _chain_tip_change,
        _tx_verifier,
        mut recent_syncs,
        _mempool_transaction_receiver,
    ) = setup(&network, cost_limit, true).await;

    service.enable(&mut recent_syncs).await;

    let insert_err = service
        .storage()
        .insert(last_transaction.clone(), Vec::new(), None)
        .expect_err("expected insert to fail for non-standard inputs");

    assert_eq!(
        insert_err,
        MempoolError::NonStandardTransaction(
            storage::NonStandardTransactionError::NonStandardInputs
        )
    );

    Ok(())
}

fn op_return_script(data: &[u8]) -> transparent::Script {
    // Build a minimal OP_RETURN script using small pushdata (<= 75 bytes).
    assert!(data.len() <= 75, "test helper only supports small pushdata");

    let mut bytes = Vec::with_capacity(2 + data.len());
    bytes.push(0x6a);
    bytes.push(data.len() as u8);
    bytes.extend_from_slice(data);
    transparent::Script::new(&bytes)
}

fn multisig_script(required: u8, key_count: usize) -> transparent::Script {
    // Construct a bare multisig output: OP_M <pubkeys> OP_N OP_CHECKMULTISIG.
    assert!(required >= 1 && required <= key_count as u8);
    assert!(key_count <= 16);

    let mut bytes = Vec::new();
    bytes.push(op_n(required));

    for i in 0..key_count {
        bytes.push(33u8);
        let mut pubkey = vec![0u8; 33];
        pubkey[0] = 0x02;
        pubkey[1] = i as u8;
        bytes.extend_from_slice(&pubkey);
    }

    bytes.push(op_n(key_count as u8));
    bytes.push(0xae);

    transparent::Script::new(&bytes)
}

fn p2pkh_script(pubkey_hash: [u8; 20]) -> transparent::Script {
    let mut bytes = Vec::with_capacity(25);
    bytes.push(0x76);
    bytes.push(0xa9);
    bytes.push(20);
    bytes.extend_from_slice(&pubkey_hash);
    bytes.push(0x88);
    bytes.push(0xac);
    transparent::Script::new(&bytes)
}

fn op_n(n: u8) -> u8 {
    if n == 0 {
        0x00
    } else {
        0x50 + n
    }
}

fn set_first_prevout_unlock_script(tx: &mut Transaction, script: transparent::Script) {
    for input in tx.inputs_mut() {
        if let transparent::Input::PrevOut { unlock_script, .. } = input {
            *unlock_script = script;
            return;
        }
    }

    panic!("missing prevout input");
}

fn pick_transaction_with_prevout(network: &Network) -> VerifiedUnminedTx {
    network
        .unmined_transactions_in_blocks(..)
        .find(|transaction| {
            transaction
                .transaction
                .transaction
                .inputs()
                .iter()
                .any(|input| matches!(input, transparent::Input::PrevOut { .. }))
        })
        .expect("missing non-coinbase transaction")
}

/// Create a new [`Mempool`] instance using mocked services.
async fn setup(
    network: &Network,
    tx_cost_limit: u64,
    should_commit_genesis_block: bool,
) -> (
    Mempool,
    MockPeerSet,
    StateService,
    ChainTipChange,
    MockTxVerifier,
    RecentSyncLengths,
    tokio::sync::broadcast::Receiver<MempoolChange>,
) {
    let mempool_config = mempool::Config {
        tx_cost_limit,
        ..Default::default()
    };

    setup_with_mempool_config(network, mempool_config, should_commit_genesis_block).await
}

async fn setup_with_mempool_config(
    network: &Network,
    mempool_config: mempool::Config,
    should_commit_genesis_block: bool,
) -> (
    Mempool,
    MockPeerSet,
    StateService,
    ChainTipChange,
    MockTxVerifier,
    RecentSyncLengths,
    tokio::sync::broadcast::Receiver<MempoolChange>,
) {
    let peer_set = MockService::build().for_unit_tests();

    // UTXO verification doesn't matter here.
    let state_config = StateConfig::ephemeral();
    let (state, _read_only_state_service, latest_chain_tip, mut chain_tip_change) =
        zebra_state::init(state_config, network, Height::MAX, 0).await;
    let mut state_service = ServiceBuilder::new().buffer(10).service(state);

    let tx_verifier = MockService::build().for_unit_tests();

    let (sync_status, recent_syncs) = SyncStatus::new();
    let (misbehavior_tx, _misbehavior_rx) = tokio::sync::mpsc::channel(1);
    let (mempool, mempool_transaction_subscriber) = Mempool::new(
        &mempool_config,
        Buffer::new(BoxService::new(peer_set.clone()), 1),
        state_service.clone(),
        Buffer::new(BoxService::new(tx_verifier.clone()), 1),
        sync_status,
        latest_chain_tip,
        chain_tip_change.clone(),
        misbehavior_tx,
    );

    let mut mempool_transaction_receiver = mempool_transaction_subscriber.subscribe();
    tokio::spawn(async move { while mempool_transaction_receiver.recv().await.is_ok() {} });

    if should_commit_genesis_block {
        let genesis_block: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES
            .zcash_deserialize_into()
            .unwrap();

        // Push the genesis block to the state
        state_service
            .ready()
            .await
            .unwrap()
            .call(zebra_state::Request::CommitCheckpointVerifiedBlock(
                genesis_block.clone().into(),
            ))
            .await
            .unwrap();

        // Wait for the chain tip update without a timeout
        chain_tip_change
            .wait_for_tip_change()
            .await
            .expect("unexpected chain tip update failure");
    }

    (
        mempool,
        peer_set,
        state_service,
        chain_tip_change,
        tx_verifier,
        recent_syncs,
        mempool_transaction_subscriber.subscribe(),
    )
}
