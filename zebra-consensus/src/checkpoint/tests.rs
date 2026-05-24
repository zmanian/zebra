//! Tests for checkpoint-based block verification

#![allow(clippy::unwrap_in_result)]

use std::{cmp::min, sync::Mutex, time::Duration};

use color_eyre::eyre::{eyre, Report};
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::time::timeout;
use tower::service_fn;
use tracing_futures::Instrument;

use zebra_chain::{
    block::{self, merkle},
    parameters::{testnet::ConfiguredActivationHeights, Network, Network::*, NetworkUpgrade},
    primitives::Halo2Proof,
    serialization::ZcashDeserialize,
    transaction::{arbitrary::insert_fake_orchard_shielded_data, LockTime, Transaction},
};

use super::*;

/// The timeout we apply to each verify future during testing.
///
/// The checkpoint verifier uses `tokio::sync::oneshot` channels as futures.
/// If the verifier doesn't send a message on the channel, any tests that
/// await the channel future will hang.
///
/// This value is set to a large value, to avoid spurious failures due to
/// high system load.
const VERIFY_TIMEOUT_SECONDS: u64 = 10;

#[tokio::test(flavor = "multi_thread")]
async fn single_item_checkpoint_list_test() -> Result<(), Report> {
    single_item_checkpoint_list().await
}

#[spandoc::spandoc]
async fn single_item_checkpoint_list() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    let block0 =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..])?;
    let hash0 = block0.hash();

    // Make a checkpoint list containing only the genesis block
    let genesis_checkpoint_list: BTreeMap<block::Height, block::Hash> =
        [(block0.coinbase_height().unwrap(), hash0)]
            .iter()
            .cloned()
            .collect();

    let state_service = zebra_state::init_test(&Mainnet).await;
    let mut checkpoint_verifier =
        CheckpointVerifier::from_list(genesis_checkpoint_list, &Mainnet, None, state_service)
            .map_err(|e| eyre!(e))?;

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    /// SPANDOC: Make sure the verifier service is ready
    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    /// SPANDOC: Set up the future for block 0
    let verify_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(block0.clone()),
    );
    /// SPANDOC: Wait for the response for block 0
    // TODO(teor || jlusby): check error kind
    let verify_response = verify_future
        .map_err(|e| eyre!(e))
        .await
        .expect("timeout should not happen")
        .expect("block should verify");

    assert_eq!(verify_response, hash0);

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        FinalCheckpoint
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        FinishedVerifying
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_item_checkpoint_list_test() -> Result<(), Report> {
    multi_item_checkpoint_list().await
}

#[spandoc::spandoc]
async fn multi_item_checkpoint_list() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    // Parse all the blocks
    let mut checkpoint_data = Vec::new();
    for b in &[
        // This list is used as a checkpoint list, and as a list of blocks to
        // verify. So it must be continuous.
        &zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..],
        &zebra_test::vectors::BLOCK_MAINNET_1_BYTES[..],
    ] {
        let block = Arc::<Block>::zcash_deserialize(*b)?;
        let hash = block.hash();
        checkpoint_data.push((block.clone(), block.coinbase_height().unwrap(), hash));
    }

    // Make a checkpoint list containing all the blocks
    let checkpoint_list: BTreeMap<block::Height, block::Hash> = checkpoint_data
        .iter()
        .map(|(_block, height, hash)| (*height, *hash))
        .collect();

    let state_service = zebra_state::init_test(&Mainnet).await;
    let mut checkpoint_verifier =
        CheckpointVerifier::from_list(checkpoint_list, &Mainnet, None, state_service)
            .map_err(|e| eyre!(e))?;

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(1)
    );

    // Now verify each block
    for (block, height, hash) in checkpoint_data {
        /// SPANDOC: Make sure the verifier service is ready
        let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;

        /// SPANDOC: Set up the future for block {?height}
        let verify_future = timeout(
            Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
            ready_verifier_service.call(block.clone()),
        );
        /// SPANDOC: Wait for the response for block {?height}
        // TODO(teor || jlusby): check error kind
        let verify_response = verify_future
            .map_err(|e| eyre!(e))
            .await
            .expect("timeout should not happen")
            .expect("future should succeed");

        assert_eq!(verify_response, hash);

        if height < checkpoint_verifier.checkpoint_list.max_height() {
            assert_eq!(
                checkpoint_verifier.previous_checkpoint_height(),
                PreviousCheckpoint(height)
            );
            assert_eq!(
                checkpoint_verifier.target_checkpoint_height(),
                WaitingForBlocks
            );
        } else {
            assert_eq!(
                checkpoint_verifier.previous_checkpoint_height(),
                FinalCheckpoint
            );
            assert_eq!(
                checkpoint_verifier.target_checkpoint_height(),
                FinishedVerifying
            );
        }
        assert_eq!(
            checkpoint_verifier.checkpoint_list.max_height(),
            block::Height(1)
        );
    }

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        FinalCheckpoint
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        FinishedVerifying
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(1)
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn same_hash_checkpoint_auth_data_variant_replaces_queued_block_today() -> Result<(), Report>
{
    let _init_guard = zebra_test::init();

    let genesis =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..])?;
    let genesis_hash = genesis.hash();

    let height1_template =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_1_BYTES[..])?;

    let mut good_tx = Transaction::V5 {
        network_upgrade: NetworkUpgrade::Nu5,
        lock_time: LockTime::min_lock_time_timestamp(),
        expiry_height: block::Height(2),
        inputs: Vec::new(),
        outputs: Vec::new(),
        sapling_shielded_data: None,
        orchard_shielded_data: None,
    };
    insert_fake_orchard_shielded_data(&mut good_tx);

    let mut bad_tx = good_tx.clone();
    let orchard_data = bad_tx
        .orchard_shielded_data_mut()
        .expect("fake Orchard data was just inserted");
    orchard_data.proof = Halo2Proof(vec![0xde, 0xad, 0xbe, 0xef]);
    orchard_data.binding_sig = [0xff; 64].into();
    for action in orchard_data.actions.iter_mut() {
        action.spend_auth_sig = [0xff; 64].into();
    }

    assert_eq!(
        good_tx.hash(),
        bad_tx.hash(),
        "ZIP-244 mined IDs do not commit to V5 authorizing data"
    );
    assert_ne!(good_tx, bad_tx, "test variants must differ in auth data");

    let good_tx = Arc::new(good_tx);
    let bad_tx = Arc::new(bad_tx);

    let good_transactions = vec![height1_template.transactions[0].clone(), good_tx];
    let bad_transactions = vec![height1_template.transactions[0].clone(), bad_tx];

    let mut height1_header = *height1_template.header;
    height1_header.merkle_root = good_transactions.iter().collect::<merkle::Root>();

    let good_height1 = Arc::new(Block {
        header: Arc::new(height1_header),
        transactions: good_transactions,
    });
    let bad_height1 = Arc::new(Block {
        header: Arc::new(height1_header),
        transactions: bad_transactions,
    });

    let height1_hash = good_height1.hash();
    assert_eq!(
        height1_hash,
        bad_height1.hash(),
        "changing only V5 auth data keeps the block header hash unchanged"
    );

    let height2_template =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_2_BYTES[..])?;
    let mut height2_header = *height2_template.header;
    height2_header.previous_block_hash = height1_hash;
    let height2 = Arc::new(Block {
        header: Arc::new(height2_header),
        transactions: height2_template.transactions.clone(),
    });
    let height2_hash = height2.hash();

    let checkpoint_list: BTreeMap<block::Height, block::Hash> = [
        (block::Height(0), genesis_hash),
        (block::Height(2), height2_hash),
    ]
    .into_iter()
    .collect();

    let commit_count = Arc::new(Mutex::new(0usize));
    let state_tip = Arc::new(Mutex::new(None));
    let state_service = service_fn({
        let commit_count = commit_count.clone();
        let state_tip = state_tip.clone();

        move |request| {
            let commit_count = commit_count.clone();
            let state_tip = state_tip.clone();

            let response: Result<zebra_state::Response, BoxError> = match request {
                zebra_state::Request::CommitCheckpointVerifiedBlock(_) => {
                    let mut commit_count = commit_count.lock().expect("mock state is not poisoned");
                    *commit_count += 1;

                    if *commit_count == 1 {
                        *state_tip.lock().expect("mock state is not poisoned") =
                            Some((block::Height(0), genesis_hash));
                        Ok(zebra_state::Response::Committed(genesis_hash))
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "mock state rejected checkpoint auth-data variant",
                        )
                        .into())
                    }
                }
                zebra_state::Request::Tip => Ok(zebra_state::Response::Tip(
                    *state_tip.lock().expect("mock state is not poisoned"),
                )),
                unexpected => panic!("unexpected checkpoint state request: {unexpected:?}"),
            };

            async move { response }
        }
    });

    let network = Network::new_regtest(
        ConfiguredActivationHeights {
            nu5: Some(1),
            ..Default::default()
        }
        .into(),
    );
    let mut checkpoint_verifier =
        CheckpointVerifier::from_list(checkpoint_list, &network, None, state_service)
            .map_err(|e| eyre!(e))?;

    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    let genesis_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(genesis),
    );
    let genesis_response = genesis_future
        .await
        .expect("timeout should not happen")
        .map_err(|e| eyre!(e))?;
    assert_eq!(genesis_response, genesis_hash);
    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        PreviousCheckpoint(block::Height(0))
    );

    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    let good_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(good_height1),
    );

    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );

    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    let bad_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(bad_height1),
    );

    let good_error = good_future
        .await
        .expect("timeout should not happen")
        .expect_err("newer same-hash auth-data variant replaces older request");
    assert!(
        matches!(
            good_error,
            VerifyCheckpointError::NewerRequest { height, hash }
                if height == block::Height(1) && hash == height1_hash
        ),
        "unexpected good variant error: {good_error:?}"
    );

    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    let height2_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(height2),
    );

    let bad_error = bad_future
        .await
        .expect("timeout should not happen")
        .expect_err("mock state rejects the replacement auth-data variant");
    assert!(
        matches!(
            bad_error,
            VerifyCheckpointError::CommitCheckpointVerified(_)
        ),
        "unexpected replacement variant error: {bad_error:?}"
    );

    let height2_error = height2_future
        .await
        .expect("timeout should not happen")
        .expect_err("mock state rejects blocks after genesis");
    assert!(
        matches!(
            height2_error,
            VerifyCheckpointError::CommitCheckpointVerified(_)
        ),
        "unexpected height 2 error: {height2_error:?}"
    );

    assert_eq!(
        *commit_count.lock().expect("mock state is not poisoned"),
        3,
        "genesis, replacement height 1 block, and height 2 block reached state"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn continuous_blockchain_no_restart() -> Result<(), Report> {
    for network in Network::iter() {
        continuous_blockchain(None, network).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn continuous_blockchain_restart() -> Result<(), Report> {
    for height in 0..zebra_test::vectors::CONTINUOUS_MAINNET_BLOCKS.len() {
        continuous_blockchain(Some(block::Height(height.try_into().unwrap())), Mainnet).await?;
    }
    for height in 0..zebra_test::vectors::CONTINUOUS_TESTNET_BLOCKS.len() {
        continuous_blockchain(
            Some(block::Height(height.try_into().unwrap())),
            Network::new_default_testnet(),
        )
        .await?;
    }
    Ok(())
}

/// Test a continuous blockchain on `network`, restarting verification at `restart_height`.
//
// This span is far too verbose for use during normal testing.
// Turn the SPANDOC: comments into doc comments to re-enable.
//#[spandoc::spandoc]
async fn continuous_blockchain(
    restart_height: Option<block::Height>,
    network: Network,
) -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    // A continuous blockchain
    let blockchain = network.blockchain_iter();

    let blockchain: Vec<_> = blockchain
        .map(|(height, b)| {
            let block = Arc::<Block>::zcash_deserialize(*b).unwrap();
            let hash = block.hash();
            let coinbase_height = block.coinbase_height().unwrap();
            assert_eq!(*height, coinbase_height.0);
            (block, coinbase_height, hash)
        })
        .collect();
    let blockchain_len = blockchain.len();

    // Use some of the blocks as checkpoints
    // We use these indexes so that we test:
    //   - checkpoints don't have to be the same length
    //   - checkpoints start at genesis
    //   - checkpoints end at the end of the range (there's no point in having extra blocks)
    let expected_max_height = block::Height((blockchain_len - 1).try_into().unwrap());
    let checkpoint_list = [
        &blockchain[0],
        &blockchain[blockchain_len / 3],
        &blockchain[blockchain_len / 2],
        &blockchain[blockchain_len - 1],
    ];
    let checkpoint_list: BTreeMap<block::Height, block::Hash> = checkpoint_list
        .iter()
        .map(|(_block, height, hash)| (*height, *hash))
        .collect();

    // SPANDOC: Verify blocks, restarting at {?restart_height} {?network}
    {
        let initial_tip = restart_height.map(|block::Height(height)| {
            (blockchain[height as usize].1, blockchain[height as usize].2)
        });
        let state_service = zebra_state::init_test(&Mainnet).await;
        let mut checkpoint_verifier = CheckpointVerifier::from_list(
            checkpoint_list,
            &network,
            initial_tip,
            state_service.clone(),
        )
        .map_err(|e| eyre!(e))?;

        // Setup checks
        if restart_height.is_some() {
            assert!(
                restart_height <= Some(checkpoint_verifier.checkpoint_list.max_height()),
                "restart heights after the final checkpoint are not supported by this test"
            );
        }
        if restart_height
            .map(|h| h == checkpoint_verifier.checkpoint_list.max_height())
            .unwrap_or(false)
        {
            assert_eq!(
                checkpoint_verifier.previous_checkpoint_height(),
                FinalCheckpoint
            );
            assert_eq!(
                checkpoint_verifier.target_checkpoint_height(),
                FinishedVerifying
            );
        } else {
            assert_eq!(
                checkpoint_verifier.previous_checkpoint_height(),
                restart_height.map(InitialTip).unwrap_or(BeforeGenesis)
            );
            assert_eq!(
                checkpoint_verifier.target_checkpoint_height(),
                WaitingForBlocks
            );
        }
        assert_eq!(
            checkpoint_verifier.checkpoint_list.max_height(),
            expected_max_height
        );

        let mut handles = FuturesUnordered::new();

        // Now verify each block
        for (block, height, _hash) in blockchain {
            // Commit directly to the state until after the (fake) restart height
            if let Some(restart_height) = restart_height {
                if height <= restart_height {
                    let mut state_service = state_service.clone();
                    // SPANDOC: Make sure the state service is ready for block {?height}
                    let ready_state_service = state_service.ready().map_err(|e| eyre!(e)).await?;

                    // SPANDOC: Add block directly to the state {?height}
                    ready_state_service
                        .call(zebra_state::Request::CommitCheckpointVerifiedBlock(
                            block.clone().into(),
                        ))
                        .await
                        .map_err(|e| eyre!(e))?;

                    // Skip verification for (fake) previous blocks
                    continue;
                }
            }

            // SPANDOC: Make sure the verifier service is ready for block {?height}
            let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;

            // SPANDOC: Set up the future for block {?height}
            let verify_future = timeout(
                Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
                ready_verifier_service.call(block.clone()),
            );

            // SPANDOC: spawn verification future in the background for block {?height}
            let handle = tokio::spawn(verify_future.in_current_span());
            handles.push(handle);

            // Execution checks
            if height < checkpoint_verifier.checkpoint_list.max_height() {
                assert_eq!(
                    checkpoint_verifier.target_checkpoint_height(),
                    WaitingForBlocks
                );
            } else {
                assert_eq!(
                    checkpoint_verifier.previous_checkpoint_height(),
                    FinalCheckpoint
                );
                assert_eq!(
                    checkpoint_verifier.target_checkpoint_height(),
                    FinishedVerifying
                );
            }
        }

        // Check that we have the correct number of verify tasks
        if let Some(block::Height(restart_height)) = restart_height {
            let restart_height = restart_height as usize;
            if restart_height == blockchain_len - 1 {
                assert_eq!(
                    handles.len(),
                    0,
                    "unexpected number of verify tasks for restart height: {restart_height:?}",
                );
            } else {
                assert_eq!(
                    handles.len(),
                    blockchain_len - restart_height - 1,
                    "unexpected number of verify tasks for restart height: {restart_height:?}",
                );
            }
        } else {
            assert_eq!(
                handles.len(),
                blockchain_len,
                "unexpected number of verify tasks with no restart height",
            );
        }

        // SPANDOC: wait on spawned verification tasks for restart height {?restart_height} {?network}
        while let Some(result) = handles.next().await {
            result??.map_err(|e| eyre!(e))?;
        }

        // Final checks
        assert_eq!(
            checkpoint_verifier.previous_checkpoint_height(),
            FinalCheckpoint,
            "unexpected previous checkpoint for restart height: {restart_height:?}",
        );
        assert_eq!(
            checkpoint_verifier.target_checkpoint_height(),
            FinishedVerifying,
            "unexpected target checkpoint for restart height: {restart_height:?}",
        );
        assert_eq!(
            checkpoint_verifier.checkpoint_list.max_height(),
            expected_max_height,
            "unexpected max checkpoint height for restart height: {restart_height:?}",
        );
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn block_higher_than_max_checkpoint_fail_test() -> Result<(), Report> {
    block_higher_than_max_checkpoint_fail().await
}

#[spandoc::spandoc]
async fn block_higher_than_max_checkpoint_fail() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    let block0 =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..])?;
    let block415000 =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_415000_BYTES[..])?;

    // Make a checkpoint list containing only the genesis block
    let genesis_checkpoint_list: BTreeMap<block::Height, block::Hash> =
        [(block0.coinbase_height().unwrap(), block0.as_ref().into())]
            .iter()
            .cloned()
            .collect();

    let state_service = zebra_state::init_test(&Mainnet).await;
    let mut checkpoint_verifier =
        CheckpointVerifier::from_list(genesis_checkpoint_list, &Mainnet, None, state_service)
            .map_err(|e| eyre!(e))?;

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    /// SPANDOC: Make sure the verifier service is ready
    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    /// SPANDOC: Set up the future for block 415000
    let verify_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(block415000.clone()),
    );
    /// SPANDOC: Wait for the response for block 415000, and expect failure
    // TODO(teor || jlusby): check error kind
    let _ = verify_future
        .map_err(|e| eyre!(e))
        .await
        .expect("timeout should not happen")
        .expect_err("bad block hash should fail");

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_checkpoint_hash_fail_test() -> Result<(), Report> {
    wrong_checkpoint_hash_fail().await
}

#[spandoc::spandoc]
async fn wrong_checkpoint_hash_fail() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    let good_block0 =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..])?;
    let good_block0_hash = good_block0.hash();

    // Change the header hash
    let mut bad_block0 = good_block0.clone();
    let bad_block0_mut = Arc::make_mut(&mut bad_block0);
    Arc::make_mut(&mut bad_block0_mut.header).version = 5;

    // Make a checkpoint list containing the genesis block checkpoint
    let genesis_checkpoint_list: BTreeMap<block::Height, block::Hash> =
        [(good_block0.coinbase_height().unwrap(), good_block0_hash)]
            .iter()
            .cloned()
            .collect();

    let state_service = zebra_state::init_test(&Mainnet).await;
    let mut checkpoint_verifier =
        CheckpointVerifier::from_list(genesis_checkpoint_list, &Mainnet, None, state_service)
            .map_err(|e| eyre!(e))?;

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    /// SPANDOC: Make sure the verifier service is ready (1/3)
    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    /// SPANDOC: Set up the future for bad block 0 (1/3)
    // TODO(teor || jlusby): check error kind
    let bad_verify_future_1 = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(bad_block0.clone()),
    );
    // We can't await the future yet, because bad blocks aren't cleared
    // until the chain is verified

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    /// SPANDOC: Make sure the verifier service is ready (2/3)
    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    /// SPANDOC: Set up the future for bad block 0 again (2/3)
    // TODO(teor || jlusby): check error kind
    let bad_verify_future_2 = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(bad_block0.clone()),
    );
    // We can't await the future yet, because bad blocks aren't cleared
    // until the chain is verified

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    /// SPANDOC: Make sure the verifier service is ready (3/3)
    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    /// SPANDOC: Set up the future for good block 0 (3/3)
    let good_verify_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(good_block0.clone()),
    );
    /// SPANDOC: Wait for the response for good block 0, and expect success (3/3)
    // TODO(teor || jlusby): check error kind
    let verify_response = good_verify_future
        .map_err(|e| eyre!(e))
        .await
        .expect("timeout should not happen")
        .expect("future should succeed");

    assert_eq!(verify_response, good_block0_hash);

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        FinalCheckpoint
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        FinishedVerifying
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    // Now, await the bad futures, which should have completed

    /// SPANDOC: Wait for the response for block 0, and expect failure (1/3)
    // TODO(teor || jlusby): check error kind
    let _ = bad_verify_future_1
        .map_err(|e| eyre!(e))
        .await
        .expect("timeout should not happen")
        .expect_err("bad block hash should fail");

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        FinalCheckpoint
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        FinishedVerifying
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    /// SPANDOC: Wait for the response for block 0, and expect failure again (2/3)
    // TODO(teor || jlusby): check error kind
    let _ = bad_verify_future_2
        .map_err(|e| eyre!(e))
        .await
        .expect("timeout should not happen")
        .expect_err("bad block hash should fail");

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        FinalCheckpoint
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        FinishedVerifying
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(0)
    );

    Ok(())
}

#[test]
fn checkpoint_check_block_accepts_missing_funding_stream_outputs_today() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    let network = zebra_chain::parameters::testnet::Parameters::build()
        .with_disable_pow(true)
        .to_network()?;

    let block =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_1046400_BYTES[..])?;
    let mut block = Arc::try_unwrap(block).expect("block test vector should unwrap");
    let height = block
        .coinbase_height()
        .expect("block test vector should have a coinbase height");

    let coinbase = block
        .transactions
        .first()
        .expect("block test vector should have a coinbase transaction");
    let mut output = coinbase.outputs()[0].clone();
    output.value = amount::Amount::try_from(i32::MAX).expect("test amount should be valid");

    let malformed_coinbase = Transaction::V4 {
        inputs: coinbase.inputs().to_vec(),
        outputs: vec![output],
        lock_time: coinbase.lock_time().unwrap_or_else(LockTime::unlocked),
        expiry_height: block::Height(0),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    block.transactions = vec![Arc::new(malformed_coinbase)];
    Arc::make_mut(&mut block.header).merkle_root =
        block.transactions.iter().collect::<merkle::Root>();

    let block = Arc::new(block);
    let trusted_hash = block.hash();

    let expected_block_subsidy = block_subsidy(height, &network)?;
    let semantic_error =
        crate::block::check::subsidy_is_valid(&block, &network, expected_block_subsidy)
            .expect_err("full semantic subsidy validation should reject the malformed coinbase");
    assert!(
        matches!(
            semantic_error,
            BlockError::Transaction(crate::error::TransactionError::Subsidy(
                SubsidyError::FundingStreamNotFound
            ))
        ),
        "unexpected semantic subsidy error: {semantic_error:?}"
    );

    let state_service = service_fn(|_request: zebra_state::Request| async move {
        Err::<zebra_state::Response, BoxError>("state service is unused by check_block".into())
    });
    let checkpoint_verifier = CheckpointVerifier::from_list(
        [
            (block::Height(0), network.genesis_hash()),
            (height, trusted_hash),
        ],
        &network,
        None,
        state_service,
    )
    .map_err(|e| eyre!(e))?;

    checkpoint_verifier
        .check_block(block)
        .expect("checkpoint pre-check currently accepts the malformed coinbase");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn checkpoint_drop_cancel_test() -> Result<(), Report> {
    checkpoint_drop_cancel().await
}

#[spandoc::spandoc]
async fn checkpoint_drop_cancel() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    // Parse all the blocks
    let mut checkpoint_data = Vec::new();
    for b in &[
        // Continuous blocks are verified
        &zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..],
        &zebra_test::vectors::BLOCK_MAINNET_1_BYTES[..],
        // Other blocks can't verify, so they are rejected on drop
        &zebra_test::vectors::BLOCK_MAINNET_415000_BYTES[..],
        &zebra_test::vectors::BLOCK_MAINNET_434873_BYTES[..],
    ] {
        let block = Arc::<Block>::zcash_deserialize(*b)?;
        let hash = block.hash();
        checkpoint_data.push((block.clone(), block.coinbase_height().unwrap(), hash));
    }

    // Make a checkpoint list containing all the blocks
    let checkpoint_list: BTreeMap<block::Height, block::Hash> = checkpoint_data
        .iter()
        .map(|(_block, height, hash)| (*height, *hash))
        .collect();

    let state_service = zebra_state::init_test(&Mainnet).await;
    let mut checkpoint_verifier =
        CheckpointVerifier::from_list(checkpoint_list, &Mainnet, None, state_service)
            .map_err(|e| eyre!(e))?;

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert_eq!(
        checkpoint_verifier.checkpoint_list.max_height(),
        block::Height(434873)
    );

    let mut futures = Vec::new();
    // Now collect verify futures for each block
    for (block, height, hash) in checkpoint_data {
        /// SPANDOC: Make sure the verifier service is ready
        let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;

        /// SPANDOC: Set up the future for block {?height}
        let verify_future = timeout(
            Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
            ready_verifier_service.call(block.clone()),
        );

        futures.push((verify_future, height, hash));

        // Only continuous checkpoints verify
        assert_eq!(
            checkpoint_verifier.previous_checkpoint_height(),
            PreviousCheckpoint(block::Height(min(height.0, 1)))
        );
        assert_eq!(
            checkpoint_verifier.target_checkpoint_height(),
            WaitingForBlocks
        );
        assert_eq!(
            checkpoint_verifier.checkpoint_list.max_height(),
            block::Height(434873)
        );
    }

    // Now drop the verifier, to cancel the futures
    drop(checkpoint_verifier);

    for (verify_future, height, hash) in futures {
        /// SPANDOC: Check the response for block {?height}
        let verify_response = verify_future
            .map_err(|e| eyre!(e))
            .await
            .expect("timeout should not happen");

        if height <= block::Height(1) {
            let verify_hash =
                verify_response.expect("Continuous checkpoints should have succeeded before drop");
            assert_eq!(verify_hash, hash);
        } else {
            // TODO(teor || jlusby): check error kind
            verify_response.expect_err("Pending futures should fail on drop");
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn hard_coded_mainnet_test() -> Result<(), Report> {
    hard_coded_mainnet().await
}

#[spandoc::spandoc]
async fn hard_coded_mainnet() -> Result<(), Report> {
    let _init_guard = zebra_test::init();

    let block0 =
        Arc::<Block>::zcash_deserialize(&zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES[..])?;
    let hash0 = block0.hash();

    let state_service = zebra_state::init_test(&Mainnet).await;
    // Use the hard-coded checkpoint list
    let mut checkpoint_verifier = CheckpointVerifier::new(&Network::Mainnet, None, state_service);

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        BeforeGenesis
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    assert!(checkpoint_verifier.checkpoint_list.max_height() > block::Height(0));

    /// SPANDOC: Make sure the verifier service is ready
    let ready_verifier_service = checkpoint_verifier.ready().map_err(|e| eyre!(e)).await?;
    /// SPANDOC: Set up the future for block 0
    let verify_future = timeout(
        Duration::from_secs(VERIFY_TIMEOUT_SECONDS),
        ready_verifier_service.call(block0.clone()),
    );
    /// SPANDOC: Wait for the response for block 0
    // TODO(teor || jlusby): check error kind
    let verify_response = verify_future
        .map_err(|e| eyre!(e))
        .await
        .expect("timeout should not happen")
        .expect("block should verify");

    assert_eq!(verify_response, hash0);

    assert_eq!(
        checkpoint_verifier.previous_checkpoint_height(),
        PreviousCheckpoint(block::Height(0))
    );
    assert_eq!(
        checkpoint_verifier.target_checkpoint_height(),
        WaitingForBlocks
    );
    // The lists will get bigger over time, so we just pick a recent height
    assert!(checkpoint_verifier.checkpoint_list.max_height() > block::Height(900_000));

    Ok(())
}
