#![no_main]

use std::sync::{Arc, OnceLock};

use libfuzzer_sys::fuzz_target;
use zebra_chain::{
    block::Block, parameters::NetworkUpgrade, serialization::ZcashDeserializeInto,
    transaction::HashType, transparent,
};
use zebra_consensus::halo2::{self, Item};

static SOURCE_ITEMS: OnceLock<Vec<Item>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let source_items = SOURCE_ITEMS.get_or_init(extract_valid_halo2_items_from_blocks);
    if source_items.is_empty() {
        return;
    }

    let source_index = usize::from(byte_at(data, 0)) % source_items.len();
    let proof_index = usize::from(u16::from_le_bytes([byte_at(data, 1), byte_at(data, 2)]));
    let proof_byte = byte_at(data, 3);

    let mutated_item = halo2::fuzz::clone_item_with_proof_byte_mutation(
        &source_items[source_index],
        proof_index,
        proof_byte,
    )
    .expect("valid Orchard proofs are non-empty");

    assert!(
        !halo2::fuzz::verify_item(mutated_item),
        "mutating a valid Orchard proof byte must make it invalid"
    );
});

fn byte_at(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or_default()
}

fn extract_valid_halo2_items_from_blocks() -> Vec<Item> {
    let mut items = Vec::new();

    for bytes in zebra_test::vectors::MAINNET_BLOCKS.values() {
        let block: Block = bytes.zcash_deserialize_into().expect("valid block");

        for tx in &block.transactions {
            if tx.orchard_shielded_data().is_none() || !tx.inputs().is_empty() {
                continue;
            }

            let all_previous_outputs: Arc<Vec<transparent::Output>> = Arc::new(Vec::new());

            let Ok(sighasher) = tx.sighasher(NetworkUpgrade::Nu5, all_previous_outputs) else {
                continue;
            };

            let Some(bundle) = sighasher.orchard_bundle() else {
                continue;
            };

            let sighash = sighasher.sighash(HashType::ALL, None);
            let item = Item::new(bundle, sighash);

            if halo2::fuzz::verify_item(item.clone()) {
                items.push(item);
            }
        }
    }

    assert!(
        !items.is_empty(),
        "NU5+ test blocks must contain valid Orchard transactions without transparent inputs"
    );

    items
}
