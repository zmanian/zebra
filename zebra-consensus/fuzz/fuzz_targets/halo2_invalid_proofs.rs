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
    let byte_index = usize::from(u16::from_le_bytes([byte_at(data, 1), byte_at(data, 2)]));
    let byte_value = byte_at(data, 3);
    let action_index = usize::from(byte_at(data, 4));
    let mutation = match byte_at(data, 5) % 3 {
        0 => halo2::fuzz::AuthDataMutation::Proof { byte_index, byte_value },
        1 => halo2::fuzz::AuthDataMutation::BindingSignature {
            byte_index,
            byte_value,
        },
        _ => halo2::fuzz::AuthDataMutation::SpendAuthSignature {
            action_index,
            byte_index,
            byte_value,
        },
    };

    let mutated_item = halo2::fuzz::clone_item_with_auth_data_mutation(
        &source_items[source_index],
        mutation,
    )
    .expect("valid Orchard auth data is non-empty");

    assert!(
        !halo2::fuzz::verify_item(mutated_item),
        "mutating valid Orchard auth data must make the item invalid"
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
