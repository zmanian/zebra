//! Fixed test vectors for the non-finalized state.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use zebra_chain::{
    amount::{NonNegative, MAX_MONEY},
    block::{self, Block, Height},
    history_tree::NonEmptyHistoryTree,
    parameters::{Network, NetworkUpgrade},
    serialization::ZcashDeserializeInto,
    transaction,
    transparent::{self, OrderedUtxo, Utxo},
    value_balance::ValueBalance,
};
use zebra_test::prelude::*;

use crate::{
    arbitrary::Prepare,
    service::{
        finalized_state::FinalizedState,
        non_finalized_state::{
            chain::UpdateWith, Chain, NonFinalizedState, MIN_DURATION_BETWEEN_BACKUP_UPDATES,
        },
        write::validate_and_commit_non_finalized,
    },
    tests::{
        setup::{new_state_with_mainnet_genesis, transaction_v4_from_coinbase},
        FakeChainHelper,
    },
    Config,
};

#[test]
fn construct_empty() {
    let _init_guard = zebra_test::init();
    let _chain = Chain::new(
        &Network::Mainnet,
        Height(0),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ValueBalance::zero(),
    );
}

#[test]
#[should_panic]
fn non_finalized_transparent_received_panics_on_high_churn_today() {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let address = transparent::Address::from_pub_key_hash(network.t_addr_kind(), [7; 20]);
    let value = MAX_MONEY
        .try_into()
        .expect("MAX_MONEY is a valid non-negative amount");
    let receipts_to_overflow = (u64::MAX / (MAX_MONEY as u64)) + 1;

    let mut chain = Chain::new(
        &network,
        Height(0),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ValueBalance::zero(),
    );

    for tx_index in 0..receipts_to_overflow {
        let mut tx_hash_bytes = [0; 32];
        tx_hash_bytes[..8].copy_from_slice(&tx_index.to_le_bytes());
        let tx_hash = transaction::Hash(tx_hash_bytes);
        let outpoint = transparent::OutPoint {
            hash: tx_hash,
            index: 0,
        };
        let output = transparent::Output::new(value, address.script());
        let ordered_utxo = OrderedUtxo {
            utxo: Utxo::from_location(output.clone(), Height(1), tx_index as usize),
            tx_index_in_block: tx_index as usize,
        };

        chain
            .update_chain_tip_with(&(
                &vec![output],
                &tx_hash,
                &HashMap::from([(outpoint, ordered_utxo.clone())]),
            ))
            .expect("synthetic output should index");

        chain
            .update_chain_tip_with(&(
                &vec![transparent::Input::PrevOut {
                    outpoint,
                    unlock_script: transparent::Script::new(&[]),
                    sequence: 0,
                }],
                &tx_hash,
                &HashMap::from([(outpoint, ordered_utxo)]),
            ))
            .expect("synthetic spend should index");
    }

    let addresses = HashSet::from([address]);

    let (_balance, _received) = chain.partial_transparent_balance_change(&addresses);
}

#[test]
fn construct_single() -> Result<()> {
    let _init_guard = zebra_test::init();
    let block: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_434873_BYTES.zcash_deserialize_into()?;

    let mut chain = Chain::new(
        &Network::Mainnet,
        Height(0),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ValueBalance::fake_populated_pool(),
    );

    chain = chain.push(block.prepare().test_with_zero_spent_utxos())?;

    assert_eq!(1, chain.blocks.len());

    Ok(())
}

#[test]
fn lower_level_commit_skips_recent_chain_height_check_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let (finalized_state, mut validating_state, genesis) = new_state_with_mainnet_genesis();
    let mut direct_state = NonFinalizedState::new(&Network::Mainnet);

    let mut height_two_child_of_genesis = genesis.block.make_fake_child().make_fake_child();
    Arc::make_mut(&mut Arc::make_mut(&mut height_two_child_of_genesis).header)
        .previous_block_hash = genesis.hash;
    let coinbase = transaction_v4_from_coinbase(&height_two_child_of_genesis.transactions[0]);
    Arc::make_mut(&mut height_two_child_of_genesis).transactions[0] = Arc::new(coinbase);

    assert_eq!(
        height_two_child_of_genesis.coinbase_height(),
        Some(Height(2))
    );
    assert_eq!(
        height_two_child_of_genesis.header.previous_block_hash,
        genesis.hash
    );

    let prepared = height_two_child_of_genesis.prepare();

    let validation_error = validate_and_commit_non_finalized(
        &finalized_state.db,
        &mut validating_state,
        prepared.clone(),
    )
    .expect_err("normal state writes reject non-sequential block heights");

    assert!(matches!(
        validation_error,
        crate::ValidateContextError::NonSequentialBlock {
            candidate_height: Height(2),
            parent_height: Height(0),
        }
    ));

    direct_state
        .commit_new_chain(prepared.clone(), &finalized_state.db)
        .expect("lower-level non-finalized commit skips the recent-chain height check today");

    assert_eq!(direct_state.best_tip(), Some((Height(2), prepared.hash)));

    Ok(())
}

#[test]
fn construct_many() -> Result<()> {
    let _init_guard = zebra_test::init();

    let mut block: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_434873_BYTES.zcash_deserialize_into()?;
    let initial_height = block
        .coinbase_height()
        .expect("Block 434873 should have its height in its coinbase tx.");
    let mut blocks = vec![];

    while blocks.len() < 100 {
        let next_block = block.make_fake_child();
        blocks.push(block);
        block = next_block;
    }

    let mut chain = Chain::new(
        &Network::Mainnet,
        (initial_height - 1).expect("Initial height should be at least 1."),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ValueBalance::fake_populated_pool(),
    );

    for block in blocks {
        chain = chain.push(block.prepare().test_with_zero_spent_utxos())?;
    }

    assert_eq!(100, chain.blocks.len());

    Ok(())
}

#[test]
fn ord_matches_work() -> Result<()> {
    let _init_guard = zebra_test::init();
    let less_block = zebra_test::vectors::BLOCK_MAINNET_434873_BYTES
        .zcash_deserialize_into::<Arc<Block>>()?
        .set_work(1);
    let more_block = less_block.clone().set_work(10);

    let mut lesser_chain = Chain::new(
        &Network::Mainnet,
        Height(0),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ValueBalance::fake_populated_pool(),
    );
    lesser_chain = lesser_chain.push(less_block.prepare().test_with_zero_spent_utxos())?;

    let mut bigger_chain = Chain::new(
        &Network::Mainnet,
        Height(0),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ValueBalance::zero(),
    );
    bigger_chain = bigger_chain.push(more_block.prepare().test_with_zero_spent_utxos())?;

    assert!(bigger_chain > lesser_chain);

    Ok(())
}

#[test]
fn best_chain_wins() -> Result<()> {
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        best_chain_wins_for_network(network)?;
    }

    Ok(())
}

fn best_chain_wins_for_network(network: Network) -> Result<()> {
    // Since the brand new FinalizedState below will pass a None history tree
    // to the NonFinalizedState, we must use pre-Heartwood blocks since
    // they won't trigger the history tree update in the NonFinalizedState.
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());

    let block2 = block1.make_fake_child().set_work(10);
    let child = block1.make_fake_child().set_work(1);

    let expected_hash = block2.hash();

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    state.commit_new_chain(block2.prepare(), &finalized_state)?;
    state.commit_new_chain(child.prepare(), &finalized_state)?;

    let best_chain = state.best_chain().unwrap();
    assert!(best_chain.height_by_hash.contains_key(&expected_hash));

    Ok(())
}

#[test]
fn finalize_pops_from_best_chain() -> Result<()> {
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        finalize_pops_from_best_chain_for_network(network)?;
    }

    Ok(())
}

fn finalize_pops_from_best_chain_for_network(network: Network) -> Result<()> {
    // Since the brand new FinalizedState below will pass a None history tree
    // to the NonFinalizedState, we must use pre-Heartwood blocks since
    // they won't trigger the history tree update in the NonFinalizedState.
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());

    let block2 = block1.make_fake_child().set_work(10);
    let child = block1.make_fake_child().set_work(1);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state.commit_new_chain(block1.clone().prepare(), &finalized_state)?;
    state.commit_block(block2.clone().prepare(), &finalized_state)?;
    state.commit_block(child.prepare(), &finalized_state)?;

    let finalized = state.finalize().inner_block();

    assert_eq!(block1, finalized);

    let finalized = state.finalize().inner_block();
    assert_eq!(block2, finalized);

    assert!(state.best_chain().is_none());

    Ok(())
}

#[test]
fn invalidate_block_removes_block_and_descendants_from_chain() -> Result<()> {
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        invalidate_block_removes_block_and_descendants_from_chain_for_network(network)?;
    }

    Ok(())
}

fn invalidate_block_removes_block_and_descendants_from_chain_for_network(
    network: Network,
) -> Result<()> {
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);
    let block3 = block2.make_fake_child().set_work(1);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state.commit_new_chain(block1.clone().prepare(), &finalized_state)?;
    state.commit_block(block2.clone().prepare(), &finalized_state)?;
    state.commit_block(block3.clone().prepare(), &finalized_state)?;

    assert_eq!(
        state
            .best_chain()
            .unwrap_or(&Arc::new(Chain::default()))
            .blocks
            .len(),
        3
    );

    let _ = state.invalidate_block(block2.hash());

    let post_invalidated_chain = state.best_chain().unwrap();

    assert_eq!(post_invalidated_chain.blocks.len(), 1);
    assert!(
        post_invalidated_chain.contains_block_hash(block1.hash()),
        "the new modified chain should contain block1"
    );

    assert!(
        !post_invalidated_chain.contains_block_hash(block2.hash()),
        "the new modified chain should not contain block2"
    );
    assert!(
        !post_invalidated_chain.contains_block_hash(block3.hash()),
        "the new modified chain should not contain block3"
    );

    let invalidated_blocks_state = &state.invalidated_blocks;

    // Find an entry in the IndexMap that contains block2 hash
    let (_, invalidated_blocks_state_descendants) = invalidated_blocks_state
        .iter()
        .find_map(|(height, blocks)| {
            assert!(
                blocks.iter().any(|block| block.hash == block2.hash()),
                "invalidated_blocks should reference the hash of block2"
            );

            if blocks.iter().any(|block| block.hash == block2.hash()) {
                Some((height, blocks))
            } else {
                None
            }
        })
        .unwrap();

    match network {
        Network::Mainnet => assert!(
            invalidated_blocks_state_descendants
                .iter()
                .any(|block| block.height == block::Height(653601)),
            "invalidated descendants should contain block3"
        ),
        Network::Testnet(_parameters) => assert!(
            invalidated_blocks_state_descendants
                .iter()
                .any(|block| block.height == block::Height(584001)),
            "invalidated descendants should contain block3"
        ),
    }

    Ok(())
}

#[test]
fn fresh_non_finalized_state_forgets_invalidated_block_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);

    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    let mut state = NonFinalizedState::new(&network);
    state.commit_new_chain(block1.clone().prepare(), &finalized_state)?;
    state.commit_block(block2.clone().prepare(), &finalized_state)?;

    state
        .invalidate_block(block2.hash())
        .expect("the block should be invalidated from the live non-finalized state");

    let live_recommit_error = state
        .commit_block(block2.clone().prepare(), &finalized_state)
        .expect_err("the live non-finalized state should remember invalidated blocks");
    assert!(
        matches!(
            live_recommit_error,
            crate::ValidateContextError::BlockPreviouslyInvalidated { block_hash }
                if block_hash == block2.hash()
        ),
        "unexpected live recommit error: {live_recommit_error:?}"
    );

    let mut fresh_state = NonFinalizedState::new(&network);
    fresh_state.commit_new_chain(block1.prepare(), &finalized_state)?;
    fresh_state
        .commit_block(block2.clone().prepare(), &finalized_state)
        .expect("a fresh non-finalized state currently forgets the previous invalidation");

    assert!(
        fresh_state
            .best_chain()
            .expect("fresh state should have a best chain")
            .contains_block_hash(block2.hash()),
        "the previously invalidated block is accepted after constructing fresh non-finalized state"
    );

    Ok(())
}

#[tokio::test]
async fn backup_restore_replays_invalidated_block_today() -> Result<()> {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    let backup_dir = tempfile::Builder::new()
        .prefix("zebra-invalidated-non-finalized-backup-cache")
        .tempdir()
        .expect("temporary directory is created successfully");
    let backup_dir_path = backup_dir.path().to_path_buf();

    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);

    let mut state = NonFinalizedState::new(&network);
    state.commit_new_chain(block1.clone().prepare(), &finalized_state)?;
    state.commit_block(block2.clone().prepare(), &finalized_state)?;
    state.write_to_backup(&backup_dir_path);

    state
        .invalidate_block(block2.hash())
        .expect("the child block should be invalidated from the live state");

    assert!(
        !state
            .best_chain()
            .expect("the parent should remain after invalidating the child")
            .contains_block_hash(block2.hash()),
        "the live non-finalized state no longer contains the invalidated child"
    );

    let (restored_state, _sender, _receiver) = NonFinalizedState::new(&network)
        .with_backup(Some(backup_dir_path), &finalized_state, true, true)
        .await;

    assert!(
        restored_state
            .best_chain()
            .expect("backup restore should recreate the stale non-finalized chain")
            .contains_block_hash(block2.hash()),
        "backup restore currently replays a block invalidated after the backup was written"
    );

    Ok(())
}

#[test]
fn reconsider_block_and_reconsider_chain_correctly_reconsiders_blocks_and_descendants() -> Result<()>
{
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        reconsider_block_inserts_block_and_descendants_into_chain_for_network(network.clone())?;
    }

    Ok(())
}

fn reconsider_block_inserts_block_and_descendants_into_chain_for_network(
    network: Network,
) -> Result<()> {
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);
    let block3 = block2.make_fake_child().set_work(1);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state.commit_new_chain(block1.clone().prepare(), &finalized_state)?;
    state.commit_block(block2.clone().prepare(), &finalized_state)?;
    state.commit_block(block3.clone().prepare(), &finalized_state)?;

    assert_eq!(
        state
            .best_chain()
            .unwrap_or(&Arc::new(Chain::default()))
            .blocks
            .len(),
        3
    );

    // Invalidate block2 to update the invalidated_blocks NonFinalizedState
    let _ = state.invalidate_block(block2.hash());

    // Perform checks to ensure the invalidated_block and descendants were added to the invalidated_block
    // state
    let post_invalidated_chain = state.best_chain().unwrap();

    assert_eq!(post_invalidated_chain.blocks.len(), 1);
    assert!(
        post_invalidated_chain.contains_block_hash(block1.hash()),
        "the new modified chain should contain block1"
    );

    assert!(
        !post_invalidated_chain.contains_block_hash(block2.hash()),
        "the new modified chain should not contain block2"
    );
    assert!(
        !post_invalidated_chain.contains_block_hash(block3.hash()),
        "the new modified chain should not contain block3"
    );

    // Reconsider block2 and check that both block2 and block3 were `reconsidered` into the
    // best chain
    state.reconsider_block(block2.hash(), &finalized_state.db)?;

    let best_chain = state.best_chain().unwrap();

    assert!(
        best_chain.contains_block_hash(block2.hash()),
        "the best chain should again contain block2"
    );
    assert!(
        best_chain.contains_block_hash(block3.hash()),
        "the best chain should again contain block3"
    );

    Ok(())
}

#[test]
#[should_panic(expected = "Chain tip block hashes are always unique")]
fn invalidating_chain_root_panics_when_removing_existing_chain_today() {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state
        .commit_new_chain(block1.clone().prepare(), &finalized_state)
        .expect("fake root block should commit to an empty non-finalized state");
    state
        .commit_block(block2.prepare(), &finalized_state)
        .expect("fake child block should extend the fake root chain");

    state
        .invalidate_block(block1.hash())
        .expect("fake root block should be present before invalidation");
}

#[test]
#[should_panic(expected = "Chain tip block hashes are always unique")]
fn invalidating_same_height_fork_tips_panics_today() {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2a = block1.make_fake_child().set_work(10);
    let block2b = block1.make_fake_child().set_work(11);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state
        .commit_new_chain(block1.prepare(), &finalized_state)
        .expect("fake root block should commit to an empty non-finalized state");
    state
        .commit_block(block2a.clone().prepare(), &finalized_state)
        .expect("first fake fork tip should extend the root chain");
    state
        .commit_block(block2b.clone().prepare(), &finalized_state)
        .expect("second fake fork tip should fork from the root chain");

    state
        .invalidate_block(block2a.hash())
        .expect("first fake fork tip should be present before invalidation");
    state
        .invalidate_block(block2b.hash())
        .expect("second fake fork tip should be present before invalidation");
}

#[test]
#[should_panic(expected = "Chain tip block hashes are always unique")]
fn reconsider_block_twice_replays_stale_invalidated_entry_today() {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);
    let block3 = block2.make_fake_child().set_work(1);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state
        .commit_new_chain(block1.prepare(), &finalized_state)
        .expect("fake root block should commit to an empty non-finalized state");
    state
        .commit_block(block2.clone().prepare(), &finalized_state)
        .expect("fake child block should extend the fake root chain");
    state
        .commit_block(block3.prepare(), &finalized_state)
        .expect("fake grandchild block should extend the fake child chain");

    state
        .invalidate_block(block2.hash())
        .expect("fake child block should be present before invalidation");
    state
        .reconsider_block(block2.hash(), &finalized_state.db)
        .expect("first reconsider should restore the invalidated child chain");

    assert!(
        state.invalidated_blocks().values().any(|blocks| {
            blocks
                .first()
                .map(|block| block.hash == block2.hash())
                .unwrap_or(false)
        }),
        "first reconsider should leave the invalidated entry in the live map"
    );

    state
        .reconsider_block(block2.hash(), &finalized_state.db)
        .expect("stale invalidated entry should still be visible to reconsider");
}

#[test]
#[should_panic(expected = "only called while blocks is populated")]
fn finalize_after_invalidating_same_root_side_chain_tip_panics_today() {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2a = block1.make_fake_child().set_work(10);
    let block3a = block2a.make_fake_child().set_work(10);
    let block2b = block1.make_fake_child().set_work(1);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state
        .commit_new_chain(block1.prepare(), &finalized_state)
        .expect("fake root block should commit to an empty non-finalized state");
    state
        .commit_block(block2a.clone().prepare(), &finalized_state)
        .expect("first fake fork should extend the root chain");
    state
        .commit_block(block3a.prepare(), &finalized_state)
        .expect("best fake fork should have an extra child");
    state
        .commit_block(block2b.clone().prepare(), &finalized_state)
        .expect("second fake fork should extend the root chain");

    state
        .invalidate_block(block2b.hash())
        .expect("side-chain block should be present before invalidation");

    let finalized = state.finalize().inner_block();
    assert_eq!(finalized.hash(), block2a.header.previous_block_hash);

    let _ = state.finalize();
}

#[test]
fn finalize_retains_invalidated_record_at_finalized_height_today() {
    let _init_guard = zebra_test::init();

    let network = Network::Mainnet;
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());
    let block2 = block1.make_fake_child().set_work(10);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state
        .commit_new_chain(block1.clone().prepare(), &finalized_state)
        .expect("fake root block should commit to an empty non-finalized state");
    state
        .commit_block(block2.prepare(), &finalized_state)
        .expect("fake child block should extend the fake root chain");

    let invalidated_root = block1.clone().prepare().test_with_zero_spent_utxos();
    let finalized_height = invalidated_root.height;
    state
        .invalidated_blocks
        .insert(finalized_height, Arc::new(vec![invalidated_root]));

    let finalized = state.finalize().inner_block();
    assert_eq!(finalized.hash(), block1.hash());
    assert!(
        state.invalidated_blocks().contains_key(&finalized_height),
        "finalize() keeps invalidated records at the height it just finalized today"
    );
}

#[test]
// This test gives full coverage for `take_chain_if`
fn commit_block_extending_best_chain_doesnt_drop_worst_chains() -> Result<()> {
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        commit_block_extending_best_chain_doesnt_drop_worst_chains_for_network(network)?;
    }

    Ok(())
}

fn commit_block_extending_best_chain_doesnt_drop_worst_chains_for_network(
    network: Network,
) -> Result<()> {
    // Since the brand new FinalizedState below will pass a None history tree
    // to the NonFinalizedState, we must use pre-Heartwood blocks since
    // they won't trigger the history tree update in the NonFinalizedState.
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());

    let block2 = block1.make_fake_child().set_work(10);
    let child1 = block1.make_fake_child().set_work(1);
    let child2 = block2.make_fake_child().set_work(1);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    assert_eq!(0, state.chain_set.len());
    state.commit_new_chain(block1.prepare(), &finalized_state)?;
    assert_eq!(1, state.chain_set.len());
    state.commit_block(block2.prepare(), &finalized_state)?;
    assert_eq!(1, state.chain_set.len());
    state.commit_block(child1.prepare(), &finalized_state)?;
    assert_eq!(2, state.chain_set.len());
    state.commit_block(child2.prepare(), &finalized_state)?;
    assert_eq!(2, state.chain_set.len());

    Ok(())
}

#[test]
fn shorter_chain_can_be_best_chain() -> Result<()> {
    let _init_guard = zebra_test::init();
    for network in Network::iter() {
        shorter_chain_can_be_best_chain_for_network(network)?;
    }
    Ok(())
}

fn shorter_chain_can_be_best_chain_for_network(network: Network) -> Result<()> {
    // Since the brand new FinalizedState below will pass a None history tree
    // to the NonFinalizedState, we must use pre-Heartwood blocks since
    // they won't trigger the history tree update in the NonFinalizedState.
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());

    let long_chain_block1 = block1.make_fake_child().set_work(1);
    let long_chain_block2 = long_chain_block1.make_fake_child().set_work(1);

    let short_chain_block = block1.make_fake_child().set_work(3);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state.commit_new_chain(block1.prepare(), &finalized_state)?;
    state.commit_block(long_chain_block1.prepare(), &finalized_state)?;
    state.commit_block(long_chain_block2.prepare(), &finalized_state)?;
    state.commit_block(short_chain_block.prepare(), &finalized_state)?;
    assert_eq!(2, state.chain_set.len());

    assert_eq!(Some(2), state.best_chain_len());

    Ok(())
}

#[test]
fn longer_chain_with_more_work_wins() -> Result<()> {
    let _init_guard = zebra_test::init();
    for network in Network::iter() {
        longer_chain_with_more_work_wins_for_network(network)?;
    }

    Ok(())
}

fn longer_chain_with_more_work_wins_for_network(network: Network) -> Result<()> {
    // Since the brand new FinalizedState below will pass a None history tree
    // to the NonFinalizedState, we must use pre-Heartwood blocks since
    // they won't trigger the history tree update in the NonFinalizedState.
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());

    let long_chain_block1 = block1.make_fake_child().set_work(1);
    let long_chain_block2 = long_chain_block1.make_fake_child().set_work(1);
    let long_chain_block3 = long_chain_block2.make_fake_child().set_work(1);
    let long_chain_block4 = long_chain_block3.make_fake_child().set_work(1);

    let short_chain_block = block1.make_fake_child().set_work(3);

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state.commit_new_chain(block1.prepare(), &finalized_state)?;
    state.commit_block(long_chain_block1.prepare(), &finalized_state)?;
    state.commit_block(long_chain_block2.prepare(), &finalized_state)?;
    state.commit_block(long_chain_block3.prepare(), &finalized_state)?;
    state.commit_block(long_chain_block4.prepare(), &finalized_state)?;
    state.commit_block(short_chain_block.prepare(), &finalized_state)?;
    assert_eq!(2, state.chain_set.len());

    assert_eq!(Some(5), state.best_chain_len());

    Ok(())
}

#[test]
fn equal_length_goes_to_more_work() -> Result<()> {
    let _init_guard = zebra_test::init();

    for network in Network::iter() {
        equal_length_goes_to_more_work_for_network(network)?;
    }

    Ok(())
}
fn equal_length_goes_to_more_work_for_network(network: Network) -> Result<()> {
    // Since the brand new FinalizedState below will pass a None history tree
    // to the NonFinalizedState, we must use pre-Heartwood blocks since
    // they won't trigger the history tree update in the NonFinalizedState.
    let block1: Arc<Block> = Arc::new(network.test_block(653599, 583999).unwrap());

    let less_work_child = block1.make_fake_child().set_work(1);
    let more_work_child = block1.make_fake_child().set_work(3);
    let expected_hash = more_work_child.hash();

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let fake_value_pool = ValueBalance::<NonNegative>::fake_populated_pool();
    finalized_state.set_finalized_value_pool(fake_value_pool);

    state.commit_new_chain(block1.prepare(), &finalized_state)?;
    state.commit_block(less_work_child.prepare(), &finalized_state)?;
    state.commit_block(more_work_child.prepare(), &finalized_state)?;
    assert_eq!(2, state.chain_set.len());

    let tip_hash = state.best_tip().unwrap().1;
    assert_eq!(expected_hash, tip_hash);

    Ok(())
}

#[test]
fn history_tree_is_updated() -> Result<()> {
    for network in Network::iter() {
        history_tree_is_updated_for_network_upgrade(network, NetworkUpgrade::Heartwood)?;
    }
    // TODO: we can't test other upgrades until we have a method for creating a FinalizedState
    // with a HistoryTree.
    Ok(())
}

fn history_tree_is_updated_for_network_upgrade(
    network: Network,
    network_upgrade: NetworkUpgrade,
) -> Result<()> {
    let blocks = network.block_map();

    let height = network_upgrade.activation_height(&network).unwrap().0;

    let prev_block = Arc::new(
        blocks
            .get(&(height - 1))
            .expect("test vector exists")
            .zcash_deserialize_into::<Block>()
            .expect("block is structurally valid"),
    );

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    state
        .commit_new_chain(prev_block.clone().prepare(), &finalized_state)
        .unwrap();

    let chain = state.best_chain().unwrap();
    if network_upgrade == NetworkUpgrade::Heartwood {
        assert!(
            chain.history_block_commitment_tree().as_ref().is_none(),
            "history tree must not exist yet"
        );
    } else {
        assert!(
            chain.history_block_commitment_tree().as_ref().is_some(),
            "history tree must already exist"
        );
    }

    // The Heartwood activation block has an all-zero commitment
    let activation_block = prev_block.make_fake_child().set_block_commitment([0u8; 32]);

    state
        .commit_block(activation_block.clone().prepare(), &finalized_state)
        .unwrap();

    let chain = state.best_chain().unwrap();
    assert!(
        chain.history_block_commitment_tree().as_ref().is_some(),
        "history tree must have been (re)created"
    );
    assert_eq!(
        chain
            .history_block_commitment_tree()
            .as_ref()
            .as_ref()
            .unwrap()
            .size(),
        1,
        "history tree must have a single node"
    );

    // To fix the commitment in the next block we must recreate the history tree
    let tree = NonEmptyHistoryTree::from_block(
        &Network::Mainnet,
        activation_block.clone(),
        &chain.sapling_note_commitment_tree_for_tip().root(),
        &chain.orchard_note_commitment_tree_for_tip().root(),
    )
    .unwrap();

    let next_block = activation_block
        .make_fake_child()
        .set_block_commitment(tree.hash().into());

    state
        .commit_block(next_block.prepare(), &finalized_state)
        .unwrap();

    assert!(
        state
            .best_chain()
            .unwrap()
            .history_block_commitment_tree()
            .as_ref()
            .is_some(),
        "history tree must still exist"
    );

    Ok(())
}

#[test]
fn commitment_is_validated() {
    for network in Network::iter() {
        commitment_is_validated_for_network_upgrade(network, NetworkUpgrade::Heartwood);
    }
    // TODO: we can't test other upgrades until we have a method for creating a FinalizedState
    // with a HistoryTree.
}

fn commitment_is_validated_for_network_upgrade(network: Network, network_upgrade: NetworkUpgrade) {
    let blocks = network.block_map();
    let height = network_upgrade.activation_height(&network).unwrap().0;

    let prev_block = Arc::new(
        blocks
            .get(&(height - 1))
            .expect("test vector exists")
            .zcash_deserialize_into::<Block>()
            .expect("block is structurally valid"),
    );

    let mut state = NonFinalizedState::new(&network);
    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    state
        .commit_new_chain(prev_block.clone().prepare(), &finalized_state)
        .unwrap();

    // The Heartwood activation block must have an all-zero commitment.
    // Test error return when committing the block with the wrong commitment
    let activation_block = prev_block.make_fake_child();
    let err = state
        .commit_block(activation_block.clone().prepare(), &finalized_state)
        .unwrap_err();
    match err {
        crate::ValidateContextError::InvalidBlockCommitment(
            zebra_chain::block::CommitmentError::InvalidChainHistoryActivationReserved { .. },
        ) => {},
        _ => panic!("Error must be InvalidBlockCommitment::InvalidChainHistoryActivationReserved instead of {err:?}"),
    };

    // Test committing the Heartwood activation block with the correct commitment
    let activation_block = activation_block.set_block_commitment([0u8; 32]);
    state
        .commit_block(activation_block.clone().prepare(), &finalized_state)
        .unwrap();

    // To fix the commitment in the next block we must recreate the history tree
    let chain = state.best_chain().unwrap();
    let tree = NonEmptyHistoryTree::from_block(
        &Network::Mainnet,
        activation_block.clone(),
        &chain.sapling_note_commitment_tree_for_tip().root(),
        &chain.orchard_note_commitment_tree_for_tip().root(),
    )
    .unwrap();

    // Test committing the next block with the wrong commitment
    let next_block = activation_block.make_fake_child();
    let err = state
        .commit_block(next_block.clone().prepare(), &finalized_state)
        .unwrap_err();
    match err {
        crate::ValidateContextError::InvalidBlockCommitment(
            zebra_chain::block::CommitmentError::InvalidChainHistoryRoot { .. },
        ) => {}
        _ => panic!(
            "Error must be InvalidBlockCommitment::InvalidChainHistoryRoot instead of {err:?}"
        ),
    };

    // Test committing the next block with the correct commitment
    let next_block = next_block.set_block_commitment(tree.hash().into());
    state
        .commit_block(next_block.prepare(), &finalized_state)
        .unwrap();
}

#[tokio::test]
async fn non_finalized_state_writes_blocks_to_and_restores_blocks_from_backup_cache() {
    let network = Network::Mainnet;

    let finalized_state = FinalizedState::new(
        &Config::ephemeral(),
        &network,
        #[cfg(feature = "elasticsearch")]
        false,
    );

    let backup_dir_path = tempfile::Builder::new()
        .prefix("zebra-non-finalized-state-backup-cache")
        .tempdir()
        .expect("temporary directory is created successfully")
        .keep();

    let (mut non_finalized_state, non_finalized_state_sender, _receiver) =
        NonFinalizedState::new(&network)
            .with_backup(
                Some(backup_dir_path.clone()),
                &finalized_state.db,
                false,
                false,
            )
            .await;

    let blocks = network.block_map();
    let height = NetworkUpgrade::Heartwood
        .activation_height(&network)
        .unwrap()
        .0;
    let block = Arc::new(
        blocks
            .get(&(height - 1))
            .expect("test vector exists")
            .zcash_deserialize_into::<Block>()
            .expect("block is structurally valid"),
    );

    non_finalized_state
        .commit_new_chain(block.into(), &finalized_state.db)
        .expect("committing test block should succeed");

    non_finalized_state_sender
        .send(non_finalized_state.clone())
        .expect("backup task should have a receiver, channel should be open");

    // Wait for the minimum update time
    tokio::time::sleep(Duration::from_secs(1) + MIN_DURATION_BETWEEN_BACKUP_UPDATES).await;

    let (non_finalized_state, _sender, _receiver) = NonFinalizedState::new(&network)
        .with_backup(Some(backup_dir_path), &finalized_state.db, true, false)
        .await;

    assert_eq!(
        non_finalized_state.best_chain_len(),
        Some(1),
        "non-finalized state should have restored the block committed \
        to the previous non-finalized state"
    );
}
