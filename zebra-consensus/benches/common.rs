//! Shared helpers for zebra-consensus benchmarks.
//!
//! Each bench file under `benches/` is its own crate, so helpers shared across
//! them live here and are wired in via `mod common;` at the top of each bench.

#![allow(dead_code)]

use std::sync::Arc;

use zebra_chain::{
    block::Block, parameters::NetworkUpgrade, serialization::ZcashDeserializeInto, transparent,
};
use zebra_consensus::halo2::Item;

/// Creates a batch of `n` items by cycling through `source`.
///
/// The source test vectors in `zebra-test` contain only a handful of real
/// proofs/bundles. To benchmark realistic batch sizes, items are repeated.
/// This is valid for cost-per-proof measurements because proof verification
/// runs the same curve operations regardless of proof content, but it does
/// not capture the memory/cache patterns of fully unique inputs.
pub fn cycled<T: Clone>(source: &[T], n: usize) -> Vec<T> {
    source.iter().cycle().take(n).cloned().collect()
}

/// Extracts valid Halo2 items (Orchard bundles + sighashes) from NU5+ mainnet
/// test blocks.
///
/// Transactions with transparent inputs are skipped because computing their
/// sighash requires the previous outputs they spend, which are not available
/// in the test vectors. Orchard-only and Sapling-to-Orchard transactions
/// work with an empty previous outputs set.
pub fn extract_halo2_items_from_blocks() -> Vec<Item> {
    let mut items = Vec::new();

    for bytes in zebra_test::vectors::MAINNET_BLOCKS.values() {
        let block: Block = bytes.zcash_deserialize_into().expect("valid block");

        for tx in &block.transactions {
            if tx.orchard_shielded_data().is_none() {
                continue;
            }

            if !tx.inputs().is_empty() {
                continue;
            }

            let all_previous_outputs: Arc<Vec<transparent::Output>> = Arc::new(Vec::new());

            let Ok(sighasher) = tx.sighasher(NetworkUpgrade::Nu5, all_previous_outputs) else {
                continue;
            };

            let Some(bundle) = sighasher.orchard_bundle() else {
                continue;
            };

            let sighash = sighasher.sighash(zebra_chain::transaction::HashType::ALL, None);

            items.push(Item::new(bundle, sighash));
        }
    }

    assert!(
        !items.is_empty(),
        "NU5+ test blocks must contain Orchard transactions without transparent inputs"
    );

    items
}
