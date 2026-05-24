//! Fixed database test vectors for blocks and transactions.
//!
//! These tests check that the database correctly serializes
//! and deserializes large heights, blocks and transactions.
//!
//! # TODO
//!
//! Test large blocks and transactions with shielded data,
//! including data activated in Overwinter and later network upgrades.
//!
//! Check transparent address indexes, UTXOs, etc.

use std::{iter, sync::Arc};

use zebra_chain::{
    amount::{Amount, NonNegative, MAX_MONEY},
    block::{
        tests::generate::{
            large_multi_transaction_block, large_single_transaction_block_many_inputs,
            large_single_transaction_block_many_outputs,
        },
        Block, Height,
    },
    parameters::Network::{self, *},
    serialization::{ZcashDeserializeInto, ZcashSerialize},
    transaction::{LockTime, Transaction},
    transparent::{self, new_ordered_outputs_with_height},
    value_balance::ValueBalance,
};
use zebra_test::vectors::{MAINNET_BLOCKS, TESTNET_BLOCKS};

use crate::{
    constants::{state_database_format_version_in_code, STATE_DATABASE_KIND},
    request::{FinalizedBlock, Treestate},
    service::finalized_state::{
        disk_db::DiskWriteBatch,
        disk_format::upgrade::{
            block_info_and_address_received::Upgrade, DbFormatChange, DiskFormatUpgrade,
        },
        ZebraDb, STATE_COLUMN_FAMILIES_IN_CODE,
    },
    CheckpointVerifiedBlock, Config, SemanticallyVerifiedBlock,
};

/// Storage round-trip test for block and transaction data in the finalized state database.
#[test]
fn test_block_db_round_trip() {
    let mainnet_test_cases = MAINNET_BLOCKS
        .values()
        .map(|block| block.zcash_deserialize_into().unwrap());
    let testnet_test_cases = TESTNET_BLOCKS
        .values()
        .map(|block| block.zcash_deserialize_into().unwrap());

    test_block_db_round_trip_with(&Mainnet, mainnet_test_cases);
    test_block_db_round_trip_with(&Network::new_default_testnet(), testnet_test_cases);

    // It doesn't matter if these blocks are mainnet or testnet,
    // because there is no validation at this level of the database.
    //
    // These blocks have the same height and header hash, so they each need a new state.
    test_block_db_round_trip_with(&Mainnet, iter::once(large_multi_transaction_block()));

    // These blocks are unstable under serialization, so we apply a round-trip first.
    //
    // TODO: fix the bug in the generated test vectors.
    let block = large_single_transaction_block_many_inputs();
    let block_data = block
        .zcash_serialize_to_vec()
        .expect("serialization to vec never fails");
    let block: Block = block_data
        .zcash_deserialize_into()
        .expect("deserialization of valid serialized block never fails");
    test_block_db_round_trip_with(&Mainnet, iter::once(block));

    let block = large_single_transaction_block_many_outputs();
    let block_data = block
        .zcash_serialize_to_vec()
        .expect("serialization to vec never fails");
    let block: Block = block_data
        .zcash_deserialize_into()
        .expect("deserialization of valid serialized block never fails");
    test_block_db_round_trip_with(&Mainnet, iter::once(block));
}

fn test_block_db_round_trip_with(
    network: &Network,
    block_test_cases: impl IntoIterator<Item = Block>,
) {
    let _init_guard = zebra_test::init();

    let state = ZebraDb::new(
        &Config::ephemeral(),
        STATE_DATABASE_KIND,
        &state_database_format_version_in_code(),
        network,
        // The raw database accesses in this test create invalid database formats.
        true,
        STATE_COLUMN_FAMILIES_IN_CODE
            .iter()
            .map(ToString::to_string),
        false,
    );

    // Check that each block round-trips to the database
    for original_block in block_test_cases.into_iter() {
        // First, check that the block round-trips without using the database
        let block_data = original_block
            .zcash_serialize_to_vec()
            .expect("serialization to vec never fails");
        let round_trip_block: Block = block_data
            .zcash_deserialize_into()
            .expect("deserialization of valid serialized block never fails");
        let round_trip_data = round_trip_block
            .zcash_serialize_to_vec()
            .expect("serialization to vec never fails");

        assert_eq!(
            original_block, round_trip_block,
            "test block structure must round-trip",
        );
        assert_eq!(
            block_data, round_trip_data,
            "test block data must round-trip",
        );

        // Now, use the database
        let original_block = Arc::new(original_block);
        let checkpoint_verified = if original_block.coinbase_height().is_some() {
            CheckpointVerifiedBlock::from(original_block.clone())
        } else {
            // Fake a zero height
            let hash = original_block.hash();
            let transaction_hashes: Arc<[_]> = original_block
                .transactions
                .iter()
                .map(|tx| tx.hash())
                .collect();
            let new_outputs =
                new_ordered_outputs_with_height(&original_block, Height(0), &transaction_hashes);

            CheckpointVerifiedBlock(SemanticallyVerifiedBlock {
                block: original_block.clone(),
                hash,
                height: Height(0),
                new_outputs,
                transaction_hashes,
                deferred_pool_balance_change: None,
            })
        };

        let dummy_treestate = Treestate::default();
        let finalized =
            FinalizedBlock::from_checkpoint_verified(checkpoint_verified, dummy_treestate);

        // Skip validation by writing the block directly to the database
        let mut batch = DiskWriteBatch::new();
        batch.prepare_block_header_and_transaction_data_batch(&state.db, &finalized);
        state.db.write(batch).expect("block is valid for writing");

        // Now read it back from the state
        let stored_block = state
            .block(finalized.height.into())
            .expect("block was stored at height");

        if stored_block != original_block {
            error!(
                "
                detailed block mismatch report:
                original: {:?}\n\
                original data: {:?}\n\
                stored: {:?}\n\
                stored data: {:?}\n\
                ",
                original_block,
                hex::encode(original_block.zcash_serialize_to_vec().unwrap()),
                stored_block,
                hex::encode(stored_block.zcash_serialize_to_vec().unwrap()),
            );
        }

        assert_eq!(stored_block, original_block);
    }
}

#[test]
fn check_open_current_marks_upgrades_finished_before_validation_panics_today() {
    let _init_guard = zebra_test::init();

    let state = ZebraDb::new(
        &Config::ephemeral(),
        STATE_DATABASE_KIND,
        &state_database_format_version_in_code(),
        &Mainnet,
        // Skip the background checker so this test can create a current-version
        // database with validator-visible missing block info.
        true,
        STATE_COLUMN_FAMILIES_IN_CODE
            .iter()
            .map(ToString::to_string),
        false,
    );

    let genesis: Arc<Block> = MAINNET_BLOCKS
        .get(&0)
        .expect("mainnet genesis block test vector should exist")
        .zcash_deserialize_into()
        .expect("mainnet genesis block should deserialize");
    let genesis_finalized = FinalizedBlock::from_checkpoint_verified(
        CheckpointVerifiedBlock::from(genesis),
        Treestate::default(),
    );

    let mut batch = DiskWriteBatch::new();
    batch.prepare_block_header_and_transaction_data_batch(&state.db, &genesis_finalized);
    state
        .db
        .write(batch)
        .expect("raw genesis data is valid for writing");

    assert!(
        !state.finished_format_upgrades(),
        "the test starts before the non-upgrade path marks format upgrades complete"
    );

    let (_cancel_sender, cancel_receiver) = crossbeam_channel::bounded(1);
    let detailed_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        DbFormatChange::format_validity_checks_detailed(&state, &cancel_receiver)
    }));

    match detailed_result {
        Ok(Ok(inner_result)) => assert!(
            inner_result.is_err(),
            "the raw current-format database is missing required detailed-format data"
        ),
        Ok(Err(_cancelled)) => panic!("validation should not be cancelled"),
        Err(_panic) => {}
    }
    assert!(
        !state.finished_format_upgrades(),
        "standalone validation should not mark format upgrades complete"
    );

    let (_cancel_sender, cancel_receiver) = crossbeam_channel::bounded(1);
    let format_check = DbFormatChange::CheckOpenCurrent {
        running_version: state_database_format_version_in_code(),
    };

    let format_check_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        format_check.run_format_change_or_check(
            &state,
            state.finalized_tip_height(),
            &cancel_receiver,
        )
    }));

    assert!(
        format_check_result.is_err(),
        "CheckOpenCurrent should eventually panic on the invalid detailed format"
    );
    assert!(
        state.finished_format_upgrades(),
        "CheckOpenCurrent marks format upgrades finished before detailed validation fails"
    );
}

#[test]
fn block_info_upgrade_persists_zero_value_pool_when_recomputed_block_value_errors_today() {
    let _init_guard = zebra_test::init();

    let state = ZebraDb::new(
        &Config::ephemeral(),
        STATE_DATABASE_KIND,
        &state_database_format_version_in_code(),
        &Mainnet,
        // The raw database access below creates an invalid historical database shape.
        true,
        STATE_COLUMN_FAMILIES_IN_CODE
            .iter()
            .map(ToString::to_string),
        false,
    );

    let genesis: Arc<Block> = MAINNET_BLOCKS
        .get(&0)
        .expect("mainnet genesis block test vector should exist")
        .zcash_deserialize_into()
        .expect("mainnet genesis block should deserialize");
    let genesis_finalized = FinalizedBlock::from_checkpoint_verified(
        CheckpointVerifiedBlock::from(genesis),
        Treestate::default(),
    );
    let mut batch = DiskWriteBatch::new();
    batch.prepare_block_header_and_transaction_data_batch(&state.db, &genesis_finalized);
    state
        .db
        .write(batch)
        .expect("raw genesis data is valid for writing");

    let height = Height(1);
    let max_money: Amount<NonNegative> = MAX_MONEY.try_into().expect("MAX_MONEY is a valid amount");
    let coinbase = Arc::new(Transaction::V1 {
        inputs: vec![transparent::Input::new_coinbase(height, vec![], None)],
        outputs: vec![
            transparent::Output::new(max_money, transparent::Script::new(&[])),
            transparent::Output::new(max_money, transparent::Script::new(&[])),
        ],
        lock_time: LockTime::unlocked(),
    });

    assert!(
        coinbase.value_balance(&Default::default()).is_err(),
        "transaction-level value balance should reject an output sum above MAX_MONEY"
    );

    let header = zebra_test::vectors::DUMMY_HEADER
        .zcash_deserialize_into()
        .expect("dummy header should deserialize");
    let block = Arc::new(Block {
        header: Arc::new(header),
        transactions: vec![coinbase],
    });
    let hash = block.hash();
    let transaction_hashes: Arc<[_]> = block.transactions.iter().map(|tx| tx.hash()).collect();
    let new_outputs = new_ordered_outputs_with_height(&block, height, &transaction_hashes);
    let finalized = FinalizedBlock::from_checkpoint_verified(
        CheckpointVerifiedBlock(SemanticallyVerifiedBlock {
            block,
            hash,
            height,
            new_outputs,
            transaction_hashes,
            deferred_pool_balance_change: None,
        }),
        Treestate::default(),
    );

    let mut batch = DiskWriteBatch::new();
    batch.prepare_block_header_and_transaction_data_batch(&state.db, &finalized);
    state
        .db
        .write(batch)
        .expect("raw block data is valid for writing");

    assert!(
        state.block_info_cf().zs_get(&height).is_none(),
        "test setup should start from an older database shape without block info"
    );

    let (_cancel_sender, cancel_receiver) = crossbeam_channel::bounded(1);
    let upgrade = Upgrade;
    upgrade
        .run(height, &state, &cancel_receiver)
        .expect("upgrade should not be cancelled");
    upgrade
        .validate(&state, &cancel_receiver)
        .expect("validation should not be cancelled")
        .expect("current validation accepts the zero-delta block info");

    let block_info = state
        .block_info_cf()
        .zs_get(&height)
        .expect("upgrade should write block info");

    assert_ne!(
        block_info,
        Default::default(),
        "serialized size keeps the block info from being treated as empty"
    );
    assert_eq!(
        *block_info.value_pools(),
        ValueBalance::zero(),
        "current upgrade converts the recomputation failure into a zero value-pool delta"
    );
}
