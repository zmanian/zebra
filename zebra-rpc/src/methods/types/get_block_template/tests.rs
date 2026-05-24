//! Tests for types and functions for the `getblocktemplate` RPC.

use zcash_keys::address::Address;
use zcash_transparent::address::TransparentAddress;

use zebra_chain::{
    amount::Amount,
    block::{self, Height},
    chain_sync_status::MockSyncStatus,
    chain_tip::mock::MockChainTip,
    parameters::{
        testnet::{self, ConfiguredActivationHeights, ConfiguredFundingStreams},
        Network,
    },
    serialization::{ZcashDeserializeInto, ZcashSerialize},
    transaction::Transaction,
    transparent::OutPoint,
    work::difficulty::ParameterDifficulty as _,
};
use zebra_node_services::mempool::TransactionDependencies;
use zebra_state::GetBlockTemplateChainInfo;

use crate::methods::types::long_poll::LongPollId;

use super::{
    check_synced_to_tip, generate_coinbase_and_roots, standard_coinbase_outputs,
    zip317::select_mempool_transactions, BlockTemplateResponse,
};

/// Tests that a minimal coinbase transaction can be generated.
#[test]
fn minimal_coinbase() -> Result<(), Box<dyn std::error::Error>> {
    let regtest = testnet::Parameters::build()
        .with_slow_start_interval(Height::MIN)
        .with_activation_heights(ConfiguredActivationHeights {
            nu6: Some(1),
            ..Default::default()
        })?
        .with_funding_streams(vec![ConfiguredFundingStreams {
            height_range: Some(Height(1)..Height(10)),
            recipients: None,
        }])
        .to_network()?;

    let outputs = standard_coinbase_outputs(
        &regtest,
        Height(1),
        &Address::from(TransparentAddress::PublicKeyHash([0x42; 20])),
        Amount::zero(),
    );

    // It should be possible to generate a coinbase tx from these params.
    Transaction::new_v5_coinbase(&regtest, Height(1), outputs, vec![])
        .zcash_serialize_to_vec()?
        // Deserialization contains checks for elementary consensus rules, which must pass.
        .zcash_deserialize_into::<Transaction>()?;

    Ok(())
}

#[test]
#[should_panic(expected = "reward calculations are valid for reasonable chain heights")]
fn standard_coinbase_outputs_panics_when_fee_plus_subsidy_exceeds_max_money_today() {
    let regtest = testnet::Parameters::build()
        .with_slow_start_interval(Height::MIN)
        .with_activation_heights(ConfiguredActivationHeights {
            nu6: Some(1),
            ..Default::default()
        })
        .expect("regtest activation heights should be valid")
        .with_funding_streams(vec![ConfiguredFundingStreams {
            height_range: Some(Height(1)..Height(10)),
            recipients: None,
        }])
        .to_network()
        .expect("regtest network should be valid");

    standard_coinbase_outputs(
        &regtest,
        Height(1),
        &Address::from(TransparentAddress::PublicKeyHash([0x42; 20])),
        zebra_chain::amount::MAX_MONEY
            .try_into()
            .expect("MAX_MONEY is a valid non-negative amount"),
    );
}

#[test]
fn generate_coinbase_and_roots_rejects_pre_canopy_custom_network() {
    let custom_testnet = pre_canopy_custom_testnet();

    let result = generate_coinbase_and_roots(
        &custom_testnet,
        Height(1),
        &Address::from(TransparentAddress::PublicKeyHash([0x42; 20])),
        &[],
        Some(block::CHAIN_HISTORY_ACTIVATION_RESERVED.into()),
        vec![],
    );

    assert_eq!(
        result.expect_err("pre-Canopy template generation should be rejected"),
        "Zebra does not support generating pre-Canopy coinbase transactions"
    );
}

#[test]
#[should_panic(expected = "coinbase should be valid under the given parameters")]
fn block_template_response_panics_for_pre_canopy_custom_network_today() {
    let custom_testnet = pre_canopy_custom_testnet();
    let now = zebra_chain::serialization::DateTime32::now();
    let chain_info = GetBlockTemplateChainInfo {
        tip_hash: custom_testnet.genesis_hash(),
        tip_height: Height::MIN,
        chain_history_root: Some(block::CHAIN_HISTORY_ACTIVATION_RESERVED.into()),
        expected_difficulty: Default::default(),
        cur_time: now,
        min_time: now,
        max_time: now,
    };

    let _ = BlockTemplateResponse::new_internal(
        &custom_testnet,
        &Address::from(TransparentAddress::PublicKeyHash([0x42; 20])),
        &chain_info,
        LongPollId::new(0, 0, 0, 0, 0),
        vec![],
        None,
        vec![],
    );
}

fn pre_canopy_custom_testnet() -> Network {
    testnet::Parameters::build()
        .with_slow_start_interval(Height::MIN)
        .with_activation_heights(ConfiguredActivationHeights {
            before_overwinter: Some(1),
            overwinter: Some(1),
            sapling: Some(1),
            blossom: Some(1),
            heartwood: Some(1),
            canopy: Some(10),
            ..Default::default()
        })
        .expect("custom activation heights should be valid")
        .clear_funding_streams()
        .to_network()
        .expect("custom testnet network should be valid")
}

#[test]
fn default_testnet_gbt_sync_check_ignores_unsynced_status_today() {
    let (latest_chain_tip, latest_chain_tip_sender) = MockChainTip::new();
    latest_chain_tip_sender.send_best_tip_height(Height(1));
    latest_chain_tip_sender.send_estimated_distance_to_network_chain_tip(100_000);

    let mut sync_status = MockSyncStatus::default();
    sync_status.set_is_close_to_tip(false);

    assert!(check_synced_to_tip(
        &Network::Mainnet,
        latest_chain_tip.clone(),
        sync_status.clone()
    )
    .is_err());
    assert!(check_synced_to_tip(
        &Network::new_default_testnet(),
        latest_chain_tip,
        sync_status
    )
    .is_ok());
}

#[test]
fn selected_dependent_transaction_has_empty_template_depends_today() {
    let network = Network::Mainnet;
    let next_block_height = Height(1_100_000);
    let miner_address = Address::from(TransparentAddress::PublicKeyHash([0x7e; 20]));
    let unmined_txs: Vec<_> = network
        .unmined_transactions_in_blocks(..)
        .filter(|tx| !tx.transaction.transaction.is_coinbase())
        .take(3)
        .collect();

    let dependent_tx = unmined_txs.first().expect("should have 3 txs");
    let dependent_tx_id = dependent_tx.transaction.id.mined_id();
    let independent_tx_id = unmined_txs
        .get(2)
        .expect("should have 3 txs")
        .transaction
        .id
        .mined_id();

    let mut mempool_tx_deps = TransactionDependencies::default();
    mempool_tx_deps.add(
        dependent_tx.transaction.id.mined_id(),
        vec![OutPoint::from_usize(independent_tx_id, 0)],
    );

    let selected_txs = select_mempool_transactions(
        &network,
        next_block_height,
        &miner_address,
        unmined_txs,
        mempool_tx_deps,
        Vec::new(),
        #[cfg(all(zcash_unstable = "nu7", feature = "tx_v6"))]
        None,
    );

    assert!(
        selected_txs
            .iter()
            .any(|(dependency_depth, tx)| *dependency_depth == 1
                && tx.transaction.id.mined_id() == dependent_tx_id),
        "the selector should include the dependent tx with dependency depth 1"
    );

    let now = zebra_chain::serialization::DateTime32::now();
    let chain_info = GetBlockTemplateChainInfo {
        tip_hash: network.genesis_hash(),
        tip_height: (next_block_height - 1).expect("test height is above the genesis block height"),
        chain_history_root: Some(block::CHAIN_HISTORY_ACTIVATION_RESERVED.into()),
        expected_difficulty: network.target_difficulty_limit().to_compact(),
        cur_time: now,
        min_time: now,
        max_time: now,
    };

    let response = BlockTemplateResponse::new_internal(
        &network,
        &miner_address,
        &chain_info,
        LongPollId::new(0, 0, 0, 0, 0),
        selected_txs,
        None,
        vec![],
    );

    let dependent_template = response
        .transactions
        .iter()
        .find(|tx_template| tx_template.hash == dependent_tx_id)
        .expect("selected dependent tx should be in the response");

    assert!(
        dependent_template.depends.is_empty(),
        "current template conversion drops dependency indexes"
    );
}
