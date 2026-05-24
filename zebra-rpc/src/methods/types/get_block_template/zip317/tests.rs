//! Tests for ZIP-317 transaction selection for block template production

#![allow(clippy::unwrap_in_result)]

use std::collections::HashSet;

use zcash_keys::address::Address;

use zcash_transparent::address::TransparentAddress;
use zebra_chain::{block::Height, parameters::Network, transaction, transparent::OutPoint};
use zebra_node_services::mempool::TransactionDependencies;

use super::{
    has_direct_dependencies_with_scan_count, select_mempool_transactions,
    take_fee_weighted_index_candidate_counts,
};

#[test]
fn excludes_tx_with_unselected_dependencies() {
    let network = Network::Mainnet;
    let next_block_height = Height(1_000_000);
    let extra_coinbase_data = Vec::new();
    let mut mempool_tx_deps = TransactionDependencies::default();
    let miner_address = Address::from(TransparentAddress::PublicKeyHash([0x7e; 20]));

    let unmined_tx = network
        .unmined_transactions_in_blocks(..)
        .next()
        .expect("should not be empty");

    mempool_tx_deps.add(
        unmined_tx.transaction.id.mined_id(),
        vec![OutPoint::from_usize(transaction::Hash([0; 32]), 0)],
    );

    assert_eq!(
        select_mempool_transactions(
            &network,
            next_block_height,
            &miner_address,
            vec![unmined_tx],
            mempool_tx_deps,
            extra_coinbase_data,
            #[cfg(all(zcash_unstable = "nu7", feature = "tx_v6"))]
            None,
        ),
        vec![],
        "should not select any transactions when dependencies are unavailable"
    );
}

#[test]
fn includes_tx_with_selected_dependencies() {
    let network = Network::Mainnet;
    let next_block_height = Height(1_000_000);
    let unmined_txs: Vec<_> = network.unmined_transactions_in_blocks(..).take(3).collect();
    let miner_address = Address::from(TransparentAddress::PublicKeyHash([0x7e; 20]));

    let dependent_tx1 = unmined_txs.first().expect("should have 3 txns");
    let dependent_tx2 = unmined_txs.get(1).expect("should have 3 txns");
    let independent_tx_id = unmined_txs
        .get(2)
        .expect("should have 3 txns")
        .transaction
        .id
        .mined_id();

    let mut mempool_tx_deps = TransactionDependencies::default();
    mempool_tx_deps.add(
        dependent_tx1.transaction.id.mined_id(),
        vec![OutPoint::from_usize(independent_tx_id, 0)],
    );
    mempool_tx_deps.add(
        dependent_tx2.transaction.id.mined_id(),
        vec![
            OutPoint::from_usize(independent_tx_id, 0),
            OutPoint::from_usize(transaction::Hash([0; 32]), 0),
        ],
    );

    let extra_coinbase_data = Vec::new();

    let selected_txs = select_mempool_transactions(
        &network,
        next_block_height,
        &miner_address,
        unmined_txs.clone(),
        mempool_tx_deps.clone(),
        extra_coinbase_data,
        #[cfg(all(zcash_unstable = "nu7", feature = "tx_v6"))]
        None,
    );

    assert_eq!(
        selected_txs.len(),
        2,
        "should select the independent transaction and 1 of the dependent txs, selected: {selected_txs:?}"
    );

    let selected_tx_by_id = |id| {
        selected_txs
            .iter()
            .find(|(_, tx)| tx.transaction.id.mined_id() == id)
    };

    let (dependency_depth, _) =
        selected_tx_by_id(independent_tx_id).expect("should select the independent tx");

    assert_eq!(
        *dependency_depth, 0,
        "should return a dependency depth of 0 for the independent tx"
    );

    let (dependency_depth, _) = selected_tx_by_id(dependent_tx1.transaction.id.mined_id())
        .expect("should select dependent_tx1");

    assert_eq!(
        *dependency_depth, 1,
        "should return a dependency depth of 1 for the dependent tx"
    );
}

#[test]
fn independent_transaction_selection_rebuilds_weighted_index_after_each_candidate_today() {
    let network = Network::Mainnet;
    let next_block_height = Height(1_000_000);
    let extra_coinbase_data = Vec::new();
    let miner_address = Address::from(TransparentAddress::PublicKeyHash([0x7e; 20]));
    let mempool_tx_deps = TransactionDependencies::default();

    let unmined_txs: Vec<_> = network
        .unmined_transactions_in_blocks(..)
        .filter(|tx| !tx.transaction.transaction.is_coinbase())
        .take(8)
        .collect();

    assert_eq!(
        unmined_txs.len(),
        8,
        "test vectors should have enough independent non-coinbase transactions"
    );

    let conventional_fee_count = unmined_txs
        .iter()
        .filter(|tx| tx.pays_conventional_fee())
        .count();
    let low_fee_count = unmined_txs.len() - conventional_fee_count;

    let mut expected_rebuild_counts = Vec::new();
    expected_rebuild_counts.extend((1..=conventional_fee_count).rev());
    expected_rebuild_counts.extend((1..=low_fee_count).rev());

    let _ = take_fee_weighted_index_candidate_counts();

    let selected_txs = select_mempool_transactions(
        &network,
        next_block_height,
        &miner_address,
        unmined_txs,
        mempool_tx_deps,
        extra_coinbase_data,
        #[cfg(all(zcash_unstable = "nu7", feature = "tx_v6"))]
        None,
    );

    assert_eq!(
        selected_txs.len(),
        8,
        "all small independent test transactions should fit in the block template"
    );
    assert_eq!(
        take_fee_weighted_index_candidate_counts(),
        expected_rebuild_counts,
        "the ZIP-317 selector rebuilds the weighted index over every remaining candidate after each choice"
    );
}

#[test]
fn multi_parent_dependency_check_repeatedly_scans_selected_transactions_today() {
    let network = Network::Mainnet;
    let unrelated_count = 12;
    let parent_count = 4;
    let unmined_txs: Vec<_> = network
        .unmined_transactions_in_blocks(..)
        .filter(|tx| !tx.transaction.transaction.is_coinbase())
        .take(unrelated_count + parent_count)
        .collect();

    assert_eq!(
        unmined_txs.len(),
        unrelated_count + parent_count,
        "test vectors should have enough non-coinbase transactions"
    );

    let parent_txs = &unmined_txs[unrelated_count..];
    let parent_ids: HashSet<_> = parent_txs
        .iter()
        .map(|tx| tx.transaction.id.mined_id())
        .collect();

    let mut selected_txs: Vec<_> = unmined_txs[..unrelated_count]
        .iter()
        .cloned()
        .map(|tx| (0, tx))
        .collect();

    let mut total_scans = 0;
    for parent_tx in parent_txs {
        selected_txs.push((0, parent_tx.clone()));

        let (all_dependencies_selected, scan_count) =
            has_direct_dependencies_with_scan_count(Some(&parent_ids), &selected_txs);
        total_scans += scan_count;

        assert_eq!(
            scan_count,
            selected_txs.len(),
            "dependency check scans the full selected transaction vector until all parents are present"
        );
        assert_eq!(
            all_dependencies_selected,
            selected_txs.len() == unrelated_count + parent_count,
            "the dependent transaction becomes available only after the final parent is selected"
        );
    }

    let expected_total_scans: usize = (1..=parent_count)
        .map(|selected_parent_count| unrelated_count + selected_parent_count)
        .sum();

    assert_eq!(
        total_scans, expected_total_scans,
        "rechecking one multi-parent dependent after unrelated selections causes repeated linear scans"
    );
}
