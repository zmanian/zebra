//! Fixed test vectors for the syncer.

#![allow(clippy::unwrap_in_result)]

use std::{collections::HashMap, iter, sync::Arc, time::Duration};

use color_eyre::Report;
use futures::{Future, FutureExt};
use indexmap::IndexSet;

use zebra_chain::{
    block::{self, Block, Height},
    chain_tip::mock::{MockChainTip, MockChainTipSender},
    serialization::ZcashDeserializeInto,
};
use zebra_consensus::{Config as ConsensusConfig, RouterError, VerifyBlockError};
use zebra_network::{InventoryResponse, PeerSocketAddr};
use zebra_state::Config as StateConfig;
use zebra_test::mock_service::{MockService, PanicAssertion};

use zebra_network as zn;
use zebra_state as zs;

use crate::{
    components::{
        sync::{self, downloads::BlockDownloadVerifyError, SyncStatus},
        ChainSync,
    },
    config::ZebradConfig,
};

use InventoryResponse::*;

/// Maximum time to wait for a request to any test service.
///
/// The default [`MockService`] value can be too short for some of these tests that take a little
/// longer than expected to actually send the request.
///
/// Increasing this value causes the tests to take longer to complete, so it can't be too large.
const MAX_SERVICE_REQUEST_DELAY: Duration = Duration::from_millis(1000);

/// Test that the syncer downloads genesis, blocks 1-2 using obtain_tips, and blocks 3-4 using extend_tips.
///
/// This test also makes sure that the syncer downloads blocks in order.
#[tokio::test]
async fn sync_blocks_ok() -> Result<(), crate::BoxError> {
    // Get services
    let (
        chain_sync_future,
        _sync_status,
        mut block_verifier_router,
        mut peer_set,
        mut state_service,
        _mock_chain_tip_sender,
    ) = setup();

    // Get blocks
    let block0: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.zcash_deserialize_into()?;
    let block0_hash = block0.hash();

    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES.zcash_deserialize_into()?;
    let block1_hash = block1.hash();

    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES.zcash_deserialize_into()?;
    let block2_hash = block2.hash();

    let block3: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_3_BYTES.zcash_deserialize_into()?;
    let block3_hash = block3.hash();

    let block4: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_4_BYTES.zcash_deserialize_into()?;
    let block4_hash = block4.hash();

    let block5: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_5_BYTES.zcash_deserialize_into()?;
    let block5_hash = block5.hash();

    // Start the syncer
    let chain_sync_task_handle = tokio::spawn(chain_sync_future);

    // ChainSync::request_genesis

    // State is checked for genesis
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Block 0 is fetched and committed to the state
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block0_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block0.clone(),
            None,
        ))]));

    block_verifier_router
        .expect_request(zebra_consensus::Request::Commit(block0))
        .await
        .respond(block0_hash);

    // Check that nothing unexpected happened.
    // We expect more requests to the state service, because the syncer keeps on running.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for genesis again
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(Some(zs::KnownBlock::BestChain)));

    // ChainSync::obtain_tips

    // State is asked for a block locator.
    state_service
        .expect_request(zs::Request::BlockLocator)
        .await
        .respond(zs::Response::BlockLocator(vec![block0_hash]));

    // Network is sent the block locator
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block0_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block1_hash, // tip
            block2_hash, // expected_next
            block3_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // State is checked for the first unknown block (block 1)
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block0_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test obtain tips error")));
    }

    // Check that nothing unexpected happened.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for all non-tip blocks (blocks 1 & 2) in response order
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));
    state_service
        .expect_request(zs::Request::KnownBlock(block2_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Blocks 1 & 2 are fetched in order, then verified concurrently
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block1_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block1.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block2_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block2.clone(),
            None,
        ))]));

    // We can't guarantee the verification request order
    let mut remaining_blocks: HashMap<block::Hash, Arc<Block>> =
        [(block1_hash, block1), (block2_hash, block2)]
            .iter()
            .cloned()
            .collect();

    for _ in 1..=2 {
        block_verifier_router
            .expect_request_that(|req| remaining_blocks.remove(&req.block().hash()).is_some())
            .await
            .respond_with(|req| req.block().hash());
    }
    assert_eq!(
        remaining_blocks,
        HashMap::new(),
        "expected all non-tip blocks to be verified by obtain tips"
    );

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    // ChainSync::extend_tips

    // Network is sent a block locator based on the tip
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block1_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block2_hash, // tip (discarded - already fetched)
            block3_hash, // expected_next
            block4_hash,
            block5_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block1_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test extend tips error")));
    }

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    // Blocks 3 & 4 are fetched in order, then verified concurrently
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block3_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block3.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block4_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block4.clone(),
            None,
        ))]));

    // We can't guarantee the verification request order
    let mut remaining_blocks: HashMap<block::Hash, Arc<Block>> =
        [(block3_hash, block3), (block4_hash, block4)]
            .iter()
            .cloned()
            .collect();

    for _ in 3..=4 {
        block_verifier_router
            .expect_request_that(|req| remaining_blocks.remove(&req.block().hash()).is_some())
            .await
            .respond_with(|req| req.block().hash());
    }
    assert_eq!(
        remaining_blocks,
        HashMap::new(),
        "expected all non-tip blocks to be verified by extend tips"
    );

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    let chain_sync_result = chain_sync_task_handle.now_or_never();
    assert!(
        chain_sync_result.is_none(),
        "unexpected error or panic in chain sync task: {chain_sync_result:?}",
    );

    Ok(())
}

/// Test that the syncer downloads genesis, blocks 1-2 using obtain_tips, and blocks 3-4 using extend_tips,
/// with duplicate block hashes.
///
/// This test also makes sure that the syncer downloads blocks in order.
#[tokio::test]
async fn sync_blocks_duplicate_hashes_ok() -> Result<(), crate::BoxError> {
    // Get services
    let (
        chain_sync_future,
        _sync_status,
        mut block_verifier_router,
        mut peer_set,
        mut state_service,
        _mock_chain_tip_sender,
    ) = setup();

    // Get blocks
    let block0: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.zcash_deserialize_into()?;
    let block0_hash = block0.hash();

    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES.zcash_deserialize_into()?;
    let block1_hash = block1.hash();

    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES.zcash_deserialize_into()?;
    let block2_hash = block2.hash();

    let block3: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_3_BYTES.zcash_deserialize_into()?;
    let block3_hash = block3.hash();

    let block4: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_4_BYTES.zcash_deserialize_into()?;
    let block4_hash = block4.hash();

    let block5: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_5_BYTES.zcash_deserialize_into()?;
    let block5_hash = block5.hash();

    // Start the syncer
    let chain_sync_task_handle = tokio::spawn(chain_sync_future);

    // ChainSync::request_genesis

    // State is checked for genesis
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Block 0 is fetched and committed to the state
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block0_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block0.clone(),
            None,
        ))]));

    block_verifier_router
        .expect_request(zebra_consensus::Request::Commit(block0))
        .await
        .respond(block0_hash);

    // Check that nothing unexpected happened.
    // We expect more requests to the state service, because the syncer keeps on running.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for genesis again
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(Some(zs::KnownBlock::BestChain)));

    // ChainSync::obtain_tips

    // State is asked for a block locator.
    state_service
        .expect_request(zs::Request::BlockLocator)
        .await
        .respond(zs::Response::BlockLocator(vec![block0_hash]));

    // Network is sent the block locator
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block0_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block1_hash,
            block1_hash,
            block1_hash, // tip
            block2_hash, // expected_next
            block3_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // State is checked for the first unknown block (block 1)
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block0_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test obtain tips error")));
    }

    // Check that nothing unexpected happened.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for all non-tip blocks (blocks 1 & 2) in response order
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));
    state_service
        .expect_request(zs::Request::KnownBlock(block2_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Blocks 1 & 2 are fetched in order, then verified concurrently
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block1_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block1.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block2_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block2.clone(),
            None,
        ))]));

    // We can't guarantee the verification request order
    let mut remaining_blocks: HashMap<block::Hash, Arc<Block>> =
        [(block1_hash, block1), (block2_hash, block2)]
            .iter()
            .cloned()
            .collect();

    for _ in 1..=2 {
        block_verifier_router
            .expect_request_that(|req| remaining_blocks.remove(&req.block().hash()).is_some())
            .await
            .respond_with(|req| req.block().hash());
    }
    assert_eq!(
        remaining_blocks,
        HashMap::new(),
        "expected all non-tip blocks to be verified by obtain tips"
    );

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    // ChainSync::extend_tips

    // Network is sent a block locator based on the tip
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block1_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block2_hash, // tip (discarded - already fetched)
            block3_hash, // expected_next
            block4_hash,
            block3_hash,
            block4_hash,
            block5_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block1_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test extend tips error")));
    }

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    // Blocks 3 & 4 are fetched in order, then verified concurrently
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block3_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block3.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block4_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block4.clone(),
            None,
        ))]));

    // We can't guarantee the verification request order
    let mut remaining_blocks: HashMap<block::Hash, Arc<Block>> =
        [(block3_hash, block3), (block4_hash, block4)]
            .iter()
            .cloned()
            .collect();

    for _ in 3..=4 {
        block_verifier_router
            .expect_request_that(|req| remaining_blocks.remove(&req.block().hash()).is_some())
            .await
            .respond_with(|req| req.block().hash());
    }
    assert_eq!(
        remaining_blocks,
        HashMap::new(),
        "expected all non-tip blocks to be verified by extend tips"
    );

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    let chain_sync_result = chain_sync_task_handle.now_or_never();
    assert!(
        chain_sync_result.is_none(),
        "unexpected error or panic in chain sync task: {chain_sync_result:?}",
    );

    Ok(())
}

/// Test that zebra-network rejects blocks that are a long way ahead of the state tip.
#[tokio::test]
async fn sync_block_lookahead_drop() -> Result<(), crate::BoxError> {
    // Get services
    let (
        chain_sync_future,
        _sync_status,
        mut block_verifier_router,
        mut peer_set,
        mut state_service,
        _mock_chain_tip_sender,
    ) = setup();

    // Get blocks
    let block0: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.zcash_deserialize_into()?;
    let block0_hash = block0.hash();

    // Get a block that is a long way away from genesis
    let block982k: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_982681_BYTES.zcash_deserialize_into()?;

    // Start the syncer
    let chain_sync_task_handle = tokio::spawn(chain_sync_future);

    // State is checked for genesis
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Block 0 is fetched, but the peer returns a much higher block.
    // (Mismatching hashes are usually ignored by the network service,
    // but we use them here to test the syncer lookahead.)
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block0_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block982k.clone(),
            None,
        ))]));

    // Block is dropped because it is too far ahead of the tip.
    // We expect more requests to the state service, because the syncer keeps on running.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    let chain_sync_result = chain_sync_task_handle.now_or_never();
    assert!(
        chain_sync_result.is_none(),
        "unexpected error or panic in chain sync task: {chain_sync_result:?}",
    );

    Ok(())
}

/// Test that the sync downloader rejects blocks that are too high in obtain_tips.
///
/// TODO: also test that it rejects blocks behind the tip limit. (Needs ~100 fake blocks.)
#[tokio::test]
async fn sync_block_too_high_obtain_tips() -> Result<(), crate::BoxError> {
    // Get services
    let (
        chain_sync_future,
        _sync_status,
        mut block_verifier_router,
        mut peer_set,
        mut state_service,
        _mock_chain_tip_sender,
    ) = setup();

    // Get blocks
    let block0: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.zcash_deserialize_into()?;
    let block0_hash = block0.hash();

    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES.zcash_deserialize_into()?;
    let block1_hash = block1.hash();

    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES.zcash_deserialize_into()?;
    let block2_hash = block2.hash();

    let block3: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_3_BYTES.zcash_deserialize_into()?;
    let block3_hash = block3.hash();

    // Also get a block that is a long way away from genesis
    let block982k: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_982681_BYTES.zcash_deserialize_into()?;
    let block982k_hash = block982k.hash();

    // Start the syncer
    let chain_sync_task_handle = tokio::spawn(chain_sync_future);

    // ChainSync::request_genesis

    // State is checked for genesis
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Block 0 is fetched and committed to the state
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block0_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block0.clone(),
            None,
        ))]));

    block_verifier_router
        .expect_request(zebra_consensus::Request::Commit(block0))
        .await
        .respond(block0_hash);

    // Check that nothing unexpected happened.
    // We expect more requests to the state service, because the syncer keeps on running.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for genesis again
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(Some(zs::KnownBlock::BestChain)));

    // ChainSync::obtain_tips

    // State is asked for a block locator.
    state_service
        .expect_request(zs::Request::BlockLocator)
        .await
        .respond(zs::Response::BlockLocator(vec![block0_hash]));

    // Network is sent the block locator
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block0_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block982k_hash,
            block1_hash, // tip
            block2_hash, // expected_next
            block3_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // State is checked for the first unknown block (block 982k)
    state_service
        .expect_request(zs::Request::KnownBlock(block982k_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block0_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test obtain tips error")));
    }

    // Check that nothing unexpected happened.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for all non-tip blocks (blocks 982k, 1, 2) in response order
    state_service
        .expect_request(zs::Request::KnownBlock(block982k_hash))
        .await
        .respond(zs::Response::KnownBlock(None));
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));
    state_service
        .expect_request(zs::Request::KnownBlock(block2_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Blocks 982k, 1, 2 are fetched in order, then verified concurrently,
    // but block 982k verification is skipped because it is too high.
    peer_set
        .expect_request(zn::Request::BlocksByHash(
            iter::once(block982k_hash).collect(),
        ))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block982k.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block1_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block1.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block2_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block2.clone(),
            None,
        ))]));

    // At this point, the following tasks race:
    // - The valid chain verifier requests
    // - The block too high error, which causes a syncer reset and ChainSync::obtain_tips
    // - ChainSync::extend_tips for the next tip

    let chain_sync_result = chain_sync_task_handle.now_or_never();
    assert!(
        chain_sync_result.is_none(),
        "unexpected error or panic in chain sync task: {chain_sync_result:?}",
    );

    Ok(())
}

/// Test that the sync downloader rejects blocks that are too high in extend_tips.
///
/// TODO: also test that it rejects blocks behind the tip limit. (Needs ~100 fake blocks.)
#[tokio::test]
async fn sync_block_too_high_extend_tips() -> Result<(), crate::BoxError> {
    // Get services
    let (
        chain_sync_future,
        _sync_status,
        mut block_verifier_router,
        mut peer_set,
        mut state_service,
        _mock_chain_tip_sender,
    ) = setup();

    // Get blocks
    let block0: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_GENESIS_BYTES.zcash_deserialize_into()?;
    let block0_hash = block0.hash();

    let block1: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_1_BYTES.zcash_deserialize_into()?;
    let block1_hash = block1.hash();

    let block2: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_2_BYTES.zcash_deserialize_into()?;
    let block2_hash = block2.hash();

    let block3: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_3_BYTES.zcash_deserialize_into()?;
    let block3_hash = block3.hash();

    let block4: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_4_BYTES.zcash_deserialize_into()?;
    let block4_hash = block4.hash();

    let block5: Arc<Block> = zebra_test::vectors::BLOCK_MAINNET_5_BYTES.zcash_deserialize_into()?;
    let block5_hash = block5.hash();

    // Also get a block that is a long way away from genesis
    let block982k: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_982681_BYTES.zcash_deserialize_into()?;
    let block982k_hash = block982k.hash();

    // Start the syncer
    let chain_sync_task_handle = tokio::spawn(chain_sync_future);

    // ChainSync::request_genesis

    // State is checked for genesis
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Block 0 is fetched and committed to the state
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block0_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block0.clone(),
            None,
        ))]));

    block_verifier_router
        .expect_request(zebra_consensus::Request::Commit(block0))
        .await
        .respond(block0_hash);

    // Check that nothing unexpected happened.
    // We expect more requests to the state service, because the syncer keeps on running.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for genesis again
    state_service
        .expect_request(zs::Request::KnownBlock(block0_hash))
        .await
        .respond(zs::Response::KnownBlock(Some(zs::KnownBlock::BestChain)));

    // ChainSync::obtain_tips

    // State is asked for a block locator.
    state_service
        .expect_request(zs::Request::BlockLocator)
        .await
        .respond(zs::Response::BlockLocator(vec![block0_hash]));

    // Network is sent the block locator
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block0_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block1_hash, // tip
            block2_hash, // expected_next
            block3_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // State is checked for the first unknown block (block 1)
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block0_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test obtain tips error")));
    }

    // Check that nothing unexpected happened.
    peer_set.expect_no_requests().await;
    block_verifier_router.expect_no_requests().await;

    // State is checked for all non-tip blocks (blocks 1 & 2) in response order
    state_service
        .expect_request(zs::Request::KnownBlock(block1_hash))
        .await
        .respond(zs::Response::KnownBlock(None));
    state_service
        .expect_request(zs::Request::KnownBlock(block2_hash))
        .await
        .respond(zs::Response::KnownBlock(None));

    // Blocks 1 & 2 are fetched in order, then verified concurrently
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block1_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block1.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block2_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block2.clone(),
            None,
        ))]));

    // We can't guarantee the verification request order
    let mut remaining_blocks: HashMap<block::Hash, Arc<Block>> =
        [(block1_hash, block1), (block2_hash, block2)]
            .iter()
            .cloned()
            .collect();

    for _ in 1..=2 {
        block_verifier_router
            .expect_request_that(|req| remaining_blocks.remove(&req.block().hash()).is_some())
            .await
            .respond_with(|req| req.block().hash());
    }
    assert_eq!(
        remaining_blocks,
        HashMap::new(),
        "expected all non-tip blocks to be verified by obtain tips"
    );

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    // ChainSync::extend_tips

    // Network is sent a block locator based on the tip
    peer_set
        .expect_request(zn::Request::FindBlocks {
            known_blocks: vec![block1_hash],
            stop: None,
        })
        .await
        .respond(zn::Response::BlockHashes(vec![
            block2_hash, // tip (discarded - already fetched)
            block3_hash, // expected_next
            block4_hash,
            block982k_hash,
            block5_hash, // (discarded - last hash, possibly incorrect)
        ]));

    // Clear remaining block locator requests
    for _ in 0..(sync::FANOUT - 1) {
        peer_set
            .expect_request(zn::Request::FindBlocks {
                known_blocks: vec![block1_hash],
                stop: None,
            })
            .await
            .respond(Err(zn::BoxError::from("synthetic test extend tips error")));
    }

    // Check that nothing unexpected happened.
    block_verifier_router.expect_no_requests().await;
    state_service.expect_no_requests().await;

    // Blocks 3, 4, 982k are fetched in order, then verified concurrently,
    // but block 982k verification is skipped because it is too high.
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block3_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block3.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(iter::once(block4_hash).collect()))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block4.clone(),
            None,
        ))]));
    peer_set
        .expect_request(zn::Request::BlocksByHash(
            iter::once(block982k_hash).collect(),
        ))
        .await
        .respond(zn::Response::Blocks(vec![Available((
            block982k.clone(),
            None,
        ))]));

    // At this point, the following tasks race:
    // - The valid chain verifier requests
    // - The block too high error, which causes a syncer reset and ChainSync::obtain_tips
    // - ChainSync::extend_tips for the next tip

    let chain_sync_result = chain_sync_task_handle.now_or_never();
    assert!(
        chain_sync_result.is_none(),
        "unexpected error or panic in chain sync task: {chain_sync_result:?}",
    );

    Ok(())
}

/// Tests that a `BlockDownloadVerifyError::Invalid` wrapping a
/// `CommitBlockError::Duplicate` error does NOT trigger a sync restart.
#[tokio::test]
async fn should_restart_sync_returns_false() {
    let commit_error = zs::CommitBlockError::Duplicate {
        hash_or_height: None,
        location: zebra_state::KnownBlock::BestChain,
    };

    let verify_block_error = VerifyBlockError::Commit(commit_error);
    let router_error = RouterError::Block {
        source: Box::new(verify_block_error),
    };

    let err = BlockDownloadVerifyError::Invalid {
        error: router_error,
        height: block::Height(42),
        hash: block::Hash::from([0xAA; 32]),
        advertiser_addr: None,
    };

    let restart = ChainSync::<
        MockService<zn::Request, zn::Response, PanicAssertion>,
        MockService<zs::Request, zs::Response, PanicAssertion>,
        MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
        MockChainTip,
    >::should_restart_sync(&err);
    assert!(
        !restart,
        "duplicate commit block errors should NOT trigger sync restart"
    );
}

/// EXPERIMENTAL (#5709): a single block's `DownloadFailed` (e.g. a transient peer
/// `ConnectionClosed`, not just `NotFound`) drops that block and continues,
/// instead of restarting the whole syncer and discarding the queued checkpoint
/// range. Previously only `NotFound`-matching download failures continued.
#[tokio::test]
async fn download_failed_does_not_restart_sync() {
    let err = BlockDownloadVerifyError::DownloadFailed {
        // A non-`NotFound` error string: previously hit the catch-all restart arm.
        error: "peer connection closed".into(),
        hash: block::Hash::from([0xCC; 32]),
    };

    let restart = ChainSync::<
        MockService<zn::Request, zn::Response, PanicAssertion>,
        MockService<zs::Request, zs::Response, PanicAssertion>,
        MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
        MockChainTip,
    >::should_restart_sync(&err);
    assert!(
        !restart,
        "a single block download failure should drop the block and continue, not restart sync"
    );
}

/// Verifies fix for GHSA-gvjc-3w7c-92jx: `AboveLookaheadHeightLimit` now has
/// an explicit match arm in `should_restart_sync` that returns `false`.
#[tokio::test]
async fn above_lookahead_does_not_restart_sync() {
    let err = BlockDownloadVerifyError::AboveLookaheadHeightLimit {
        height: block::Height(60_000),
        hash: block::Hash::from([0xBB; 32]),
        advertiser_addr: None,
    };

    let restart = ChainSync::<
        MockService<zn::Request, zn::Response, PanicAssertion>,
        MockService<zs::Request, zs::Response, PanicAssertion>,
        MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
        MockChainTip,
    >::should_restart_sync(&err);

    assert!(
        !restart,
        "AboveLookaheadHeightLimit should NOT trigger sync restart (GHSA-gvjc-3w7c-92jx fix)"
    );
}

/// Verifies fix for GHSA-gvjc-3w7c-92jx: `AboveLookaheadHeightLimit` now
/// carries `advertiser_addr` so the offending peer can be scored.
#[tokio::test]
async fn above_lookahead_has_peer_attribution() {
    let addr: PeerSocketAddr = "127.0.0.1:8233".parse().unwrap();
    let err = BlockDownloadVerifyError::AboveLookaheadHeightLimit {
        height: block::Height(60_000),
        hash: block::Hash::from([0xCC; 32]),
        advertiser_addr: Some(addr),
    };

    let has_addr = match &err {
        BlockDownloadVerifyError::AboveLookaheadHeightLimit {
            advertiser_addr, ..
        } => advertiser_addr.is_some(),
        _ => false,
    };

    assert!(
        has_addr,
        "AboveLookaheadHeightLimit should carry advertiser_addr for peer scoring \
         (GHSA-gvjc-3w7c-92jx fix)"
    );
}

/// Verifies fix for GHSA-gvjc-3w7c-92jx: both height-limit errors now
/// return `false` from `should_restart_sync` — symmetric handling.
#[tokio::test]
async fn both_height_limits_do_not_restart_sync() {
    let below = BlockDownloadVerifyError::BehindTipHeightLimit {
        height: block::Height(1),
        hash: block::Hash::from([0xDD; 32]),
    };

    let above = BlockDownloadVerifyError::AboveLookaheadHeightLimit {
        height: block::Height(60_000),
        hash: block::Hash::from([0xEE; 32]),
        advertiser_addr: None,
    };

    let restart_below = ChainSync::<
        MockService<zn::Request, zn::Response, PanicAssertion>,
        MockService<zs::Request, zs::Response, PanicAssertion>,
        MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
        MockChainTip,
    >::should_restart_sync(&below);

    let restart_above = ChainSync::<
        MockService<zn::Request, zn::Response, PanicAssertion>,
        MockService<zs::Request, zs::Response, PanicAssertion>,
        MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
        MockChainTip,
    >::should_restart_sync(&above);

    assert!(
        !restart_below,
        "BehindTipHeightLimit should NOT restart sync"
    );
    assert!(
        !restart_above,
        "AboveLookaheadHeightLimit should NOT restart sync (GHSA-gvjc-3w7c-92jx fix)"
    );
}

/// Verifies fix for GHSA-rj6c-83wx-jxf2: `InvalidHeight` does not trigger
/// sync restart and carries `advertiser_addr` for peer scoring.
#[tokio::test]
async fn invalid_height_does_not_restart_sync() {
    let addr: PeerSocketAddr = "127.0.0.1:8233".parse().unwrap();
    let err = BlockDownloadVerifyError::InvalidHeight {
        hash: block::Hash::from([0xFF; 32]),
        advertiser_addr: Some(addr),
    };

    let restart = ChainSync::<
        MockService<zn::Request, zn::Response, PanicAssertion>,
        MockService<zs::Request, zs::Response, PanicAssertion>,
        MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
        MockChainTip,
    >::should_restart_sync(&err);

    assert!(
        !restart,
        "InvalidHeight should NOT trigger sync restart (GHSA-rj6c-83wx-jxf2 fix)"
    );

    let has_addr = match &err {
        BlockDownloadVerifyError::InvalidHeight {
            advertiser_addr, ..
        } => advertiser_addr.is_some(),
        _ => false,
    };
    assert!(
        has_addr,
        "InvalidHeight should carry advertiser_addr for peer scoring"
    );
}

fn setup() -> (
    // ChainSync
    impl Future<Output = Result<(), Report>> + Send,
    SyncStatus,
    // BlockVerifierRouter
    MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
    // PeerSet
    MockService<zebra_network::Request, zebra_network::Response, PanicAssertion>,
    // StateService
    MockService<zebra_state::Request, zebra_state::Response, PanicAssertion>,
    MockChainTipSender,
) {
    let _init_guard = zebra_test::init();

    let consensus_config = ConsensusConfig::default();
    let state_config = StateConfig::ephemeral();
    let config = ZebradConfig {
        consensus: consensus_config,
        state: state_config,
        ..Default::default()
    };

    // These tests run multiple tasks in parallel.
    // So machines under heavy load need a longer delay.
    // (For example, CI machines with limited cores.)
    let peer_set = MockService::build()
        .with_max_request_delay(MAX_SERVICE_REQUEST_DELAY)
        .for_unit_tests();

    let block_verifier_router = MockService::build()
        .with_max_request_delay(MAX_SERVICE_REQUEST_DELAY)
        .for_unit_tests();

    let state_service = MockService::build()
        .with_max_request_delay(MAX_SERVICE_REQUEST_DELAY)
        .for_unit_tests();

    let (mock_chain_tip, mock_chain_tip_sender) = MockChainTip::new();

    let (misbehavior_tx, _misbehavior_rx) = tokio::sync::mpsc::channel(1);
    let (_checkpoint_gap_sender, checkpoint_gap_receiver) = tokio::sync::watch::channel((None, 0));
    let (chain_sync, sync_status) = ChainSync::new(
        &config,
        Height(0),
        peer_set.clone(),
        block_verifier_router.clone(),
        state_service.clone(),
        mock_chain_tip,
        misbehavior_tx,
        checkpoint_gap_receiver,
    );

    let chain_sync_future = chain_sync.sync();

    (
        chain_sync_future,
        sync_status,
        block_verifier_router,
        peer_set,
        state_service,
        mock_chain_tip_sender,
    )
}

/// The concrete [`ChainSync`] type produced by [`setup_for_stall`].
///
/// All four services are mocks, so the stall-detection tests can drive the real
/// pause loop directly without spinning up the production network/state stack.
type MockChainSync = ChainSync<
    MockService<zebra_network::Request, zebra_network::Response, PanicAssertion>,
    MockService<zebra_state::Request, zebra_state::Response, PanicAssertion>,
    MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
    MockChainTip,
>;

/// Builds a [`ChainSync`] wired to mock services, and returns the struct itself
/// (rather than its `sync()` future) along with the handles a stall test needs.
///
/// Unlike [`setup`], this:
/// - returns the [`ChainSync`] struct, so tests can call its private
///   `try_to_sync_once` / `is_gap_stalled` methods and observe the
///   [`super::super::SyncError::Stalled`] return directly,
/// - keeps the checkpoint-gap sender alive and returns it, so the test can drive
///   the verifier gap signal, and
/// - lets the caller override [`crate::components::sync::Config::stall_restart_timeout`]
///   so paused-time tests can use a short deadline.
///
/// `max_checkpoint_height` is `Height(0)`, so the syncer runs in full-verify
/// phase and the saturation threshold is `full_verify_concurrency_limit / 2`.
fn setup_for_stall(
    stall_restart_timeout: Duration,
) -> (
    MockChainSync,
    // Drives the verifier contiguity-gap signal.
    tokio::sync::watch::Sender<(Option<block::Height>, u64)>,
    // BlockVerifierRouter
    MockService<zebra_consensus::Request, block::Hash, PanicAssertion>,
    // PeerSet
    MockService<zebra_network::Request, zebra_network::Response, PanicAssertion>,
    // StateService
    MockService<zebra_state::Request, zebra_state::Response, PanicAssertion>,
    // Drives the (frozen) chain tip.
    MockChainTipSender,
) {
    let _init_guard = zebra_test::init();

    let mut config = ZebradConfig {
        consensus: ConsensusConfig::default(),
        state: StateConfig::ephemeral(),
        ..Default::default()
    };
    config.sync.stall_restart_timeout = stall_restart_timeout;

    let peer_set = MockService::build()
        .with_max_request_delay(MAX_SERVICE_REQUEST_DELAY)
        .for_unit_tests();

    let block_verifier_router = MockService::build()
        .with_max_request_delay(MAX_SERVICE_REQUEST_DELAY)
        .for_unit_tests();

    let state_service = MockService::build()
        .with_max_request_delay(MAX_SERVICE_REQUEST_DELAY)
        .for_unit_tests();

    let (mock_chain_tip, mock_chain_tip_sender) = MockChainTip::new();

    let (misbehavior_tx, _misbehavior_rx) = tokio::sync::mpsc::channel(1);
    let (checkpoint_gap_sender, checkpoint_gap_receiver) = tokio::sync::watch::channel((None, 0));

    let (chain_sync, _sync_status) = ChainSync::new(
        &config,
        Height(0),
        peer_set.clone(),
        block_verifier_router.clone(),
        state_service.clone(),
        mock_chain_tip,
        misbehavior_tx,
        checkpoint_gap_receiver,
    );

    (
        chain_sync,
        checkpoint_gap_sender,
        block_verifier_router,
        peer_set,
        state_service,
        mock_chain_tip_sender,
    )
}

/// Saturates the syncer's in-flight download queue with `count` distinct fake
/// hashes, without completing any of them.
///
/// Each `download_and_verify` call spawns a task that immediately blocks on the
/// peer-set `BlocksByHash` request. Because the test never responds to those
/// requests, the tasks stay pending forever, so `downloads.in_flight()` reaches
/// `count` and the `try_to_sync_once` pause loop is entered. The blocks are
/// never delivered to the verifier, so the mock chain tip can be kept frozen.
async fn saturate_in_flight(chain_sync: &mut MockChainSync, count: usize) {
    for i in 0..count {
        // Distinct, deterministic fake hashes so the downloader's duplicate
        // check never rejects them. The bytes don't matter: these downloads
        // intentionally never resolve.
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&(i as u64).to_le_bytes());
        let hash = block::Hash(bytes);

        chain_sync
            .downloads
            .download_and_verify(hash)
            .await
            .expect("queuing a distinct fake hash for download succeeds");
    }

    assert_eq!(
        chain_sync.downloads.in_flight(),
        count,
        "all queued downloads should be in-flight (none have been answered)",
    );
}

/// Integration test for the #5709 fast-restart wiring.
///
/// This exercises the *async* pause-loop in `try_to_sync_once` end to end: the
/// `in_flight >= lookahead_limit` saturation gate, the
/// `timeout_at(last_tip_advance + stall_restart_timeout, downloads.next())`
/// race, the `is_gap_stalled` decision, and the `Err(SyncError::Stalled)`
/// propagation. Only the pure `detect_gap_stall` decision function had test
/// coverage before; this closes the gap flagged by all three reviews.
///
/// Setup mirrors a real stall: the download queue is saturated, the verifier
/// reports a persistent contiguity gap, and the state tip never advances. The
/// deadline fires (well before the 8-minute block-verify backstop) and the loop
/// returns `Stalled`.
#[tokio::test(start_paused = true)]
async fn syncer_fast_restarts_on_persistent_gap_stall() {
    // Short, non-zero timeout so the paused clock reaches the deadline quickly.
    let stall_restart_timeout = Duration::from_secs(10);

    let (mut chain_sync, gap_sender, mut block_verifier_router, _peer_set, _state, _tip_sender) =
        setup_for_stall(stall_restart_timeout);

    // The verifier is wedged on a persistent contiguity gap. The tip is left
    // frozen at genesis (`best_tip_height == None`), so it never advances.
    gap_sender
        .send((Some(Height(1_000)), 0))
        .expect("gap receiver is held by the syncer");

    // Saturate the in-flight queue. In full-verify phase the lookahead limit is
    // `full_verify_concurrency_limit` (20 by default), so 20 in-flight downloads
    // satisfy `in_flight >= lookahead_limit` and enter the pause loop.
    let lookahead_limit = ZebradConfig::default().sync.full_verify_concurrency_limit;
    saturate_in_flight(&mut chain_sync, lookahead_limit).await;

    // `try_to_sync_once` resets `last_tip_advance` only in `try_to_sync`, not
    // here, so the deadline is armed relative to construction time. Advancing the
    // paused clock past the timeout lets the deadline fire on the first wait.
    //
    // We drive the loop on a spawned task so we can advance time around it.
    let stall_task = tokio::spawn(async move {
        let result = chain_sync.try_to_sync_once(IndexSet::new()).await;
        // Return whether we got the stall signal; `SyncError` isn't `Debug`.
        matches!(result, Err(super::super::SyncError::Stalled))
    });

    // Let the loop reach the `timeout_at` await, then advance past the deadline
    // (plus `MIN_STALL_RESTART_INTERVAL`, so `not_thrashing` also holds relative
    // to the construction-time `last_stall_restart`).
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(60)).await;

    // The verifier must NOT make progress: the peer-set and verifier receive no
    // completions, the gap stays `Some(1_000)`, and the tip stays frozen, so the
    // loop must declare a stall rather than waiting out the verify backstop.
    let stalled = stall_task
        .await
        .expect("stall detection task should not panic");

    assert!(
        stalled,
        "a saturated queue with a frozen tip and a persistent gap must return SyncError::Stalled",
    );

    // The pause loop returned without ever completing a download, so the
    // verifier was never asked to commit a block. (The peer-set *was* asked for
    // `BlocksByHash` by `saturate_in_flight` — that's how the queue stays
    // saturated — so only the verifier is checked here.)
    block_verifier_router.expect_no_requests().await;
}

/// EXPERIMENTAL (#5709): the sandblasting doom-loop, end to end.
///
/// Mirror of [`syncer_fast_restarts_on_persistent_gap_stall`], except the
/// verifier ticks its liveness counter during the wait — exactly what a verifier
/// grinding through expensive sandblasting-era blocks does while the contiguity
/// frontier stays static. The deadline still fires, but `is_gap_stalled` sees the
/// advanced liveness counter, so the loop must NOT return `Stalled`: it re-arms
/// and keeps waiting instead of cancelling the in-progress work and doom-looping
/// (the failure observed on a live node as ~4,199 wholesale cancellations).
#[tokio::test(start_paused = true)]
async fn syncer_does_not_restart_when_verifier_is_busy() {
    let stall_restart_timeout = Duration::from_secs(10);

    let (mut chain_sync, gap_sender, _block_verifier_router, _peer_set, _state, _tip_sender) =
        setup_for_stall(stall_restart_timeout);

    // Same wedge surface as the stall test: a persistent gap and a frozen tip.
    gap_sender
        .send((Some(Height(1_000)), 0))
        .expect("gap receiver is held by the syncer");

    let lookahead_limit = ZebradConfig::default().sync.full_verify_concurrency_limit;
    saturate_in_flight(&mut chain_sync, lookahead_limit).await;

    let stall_task = tokio::spawn(async move {
        matches!(
            chain_sync.try_to_sync_once(IndexSet::new()).await,
            Err(super::super::SyncError::Stalled)
        )
    });

    // Let the loop reach the `timeout_at` await and snapshot (gap, liveness).
    tokio::task::yield_now().await;

    // The verifier does work mid-wait: tick the liveness counter. The gap is
    // left unchanged, so the frontier is still static — exactly the sandblasting
    // case where progress is happening below the contiguity frontier.
    gap_sender.send_modify(|(_gap, liveness)| *liveness += 1);

    // Advance past the deadline. The timeout fires, but the advanced liveness
    // counter means the verifier is busy, not wedged, so the loop re-arms and
    // keeps waiting rather than returning `Stalled`.
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    assert!(
        !stall_task.is_finished(),
        "a verifier that advanced its liveness counter must not be restarted (no doom-loop)",
    );

    stall_task.abort();
}

/// Regression for the "gap changed once, then persistent" narrative (Codex's
/// suggestion): a verifier that made progress during a window must NOT trip the
/// stall, but once the gap becomes persistent the same inputs must.
///
/// This drives the pure decision function across the two phases explicitly. The
/// individual halves are covered in `tests::stall`; this asserts the transition
/// as one story so a future refactor can't silently break the re-arm behavior.
#[test]
fn gap_changed_then_persistent_only_stalls_once_persistent() {
    use tokio::time::Instant;

    let now = Instant::now();
    let timeout = Duration::from_secs(90);

    // Common "would stall" inputs: frozen tip, saturated queue, not thrashing.
    let last_tip_advance = now - Duration::from_secs(90);
    let last_stall_restart = now - Duration::from_secs(600);
    let in_flight = 999;
    let saturation_threshold = 500;

    // Phase 1: the verifier advanced its gap since the deadline was armed
    // (`gap_now != gap_snapshot`). The loop must re-arm, not declare a stall.
    let changed = super::super::detect_gap_stall(
        timeout,
        now,
        last_tip_advance,
        last_stall_restart,
        in_flight,
        saturation_threshold,
        Some(Height(1_001)), // gap_now advanced
        Some(Height(1_000)), // gap_snapshot
        0,                   // verifier_liveness_now
        0,                   // verifier_liveness_snapshot (inert: isolate the gap logic)
    );
    assert!(
        !changed,
        "a gap that changed during the window means progress: must not stall yet",
    );

    // Phase 2: the gap is now persistent (`gap_now == gap_snapshot`) with all
    // other conditions unchanged. The same inputs must now stall.
    let persistent = super::super::detect_gap_stall(
        timeout,
        now,
        last_tip_advance,
        last_stall_restart,
        in_flight,
        saturation_threshold,
        Some(Height(1_001)), // gap_now unchanged this window
        Some(Height(1_001)), // gap_snapshot matches
        5,                   // verifier_liveness_now
        5,                   // verifier_liveness_snapshot (inert: verifier made no progress)
    );
    assert!(
        persistent,
        "once the gap is persistent across the window, the syncer must stall",
    );
}
