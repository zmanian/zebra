#![no_main]

use std::sync::{Arc, OnceLock};

use libfuzzer_sys::fuzz_target;
use tower_batch_control::RequestWeight;
use zebra_chain::{
    block::Block, parameters::NetworkUpgrade, serialization::ZcashDeserializeInto,
    transaction::HashType, transparent,
};
use zebra_consensus::{
    config::{Halo2AccelBackend, Halo2AccelConfig, Halo2AccelMode},
    halo2::{self, Item},
};

const MAX_BATCH_ITEMS: usize = 8;

static SOURCE_ITEMS: OnceLock<Vec<Item>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let source_items = SOURCE_ITEMS.get_or_init(extract_halo2_items_from_blocks);
    if source_items.is_empty() {
        return;
    }

    let config = accel_config_from_bytes(data);
    let batch_len = usize::from(byte_at(data, 0) % MAX_BATCH_ITEMS as u8) + 1;
    let mut items = Vec::with_capacity(batch_len);

    for offset in 0..batch_len {
        let source_index = usize::from(byte_at(data, 5 + offset)) % source_items.len();
        items.push(halo2::fuzz::clone_item_with_accel_config(
            &source_items[source_index],
            config.clone(),
        ));
    }

    let summary = halo2::fuzz::summarize_batch_items(&items);
    let expected_actions = items
        .iter()
        .map(RequestWeight::request_weight)
        .sum::<usize>();

    assert_eq!(summary.batch_actions, expected_actions);
    assert!(summary.candidate_items <= items.len());
    assert_eq!(summary.has_accel_candidate, summary.candidate_items > 0);

    if !config.enabled || config.mode == Halo2AccelMode::Cpu {
        assert_eq!(summary.candidate_items, 0);
        assert!(!summary.has_accel_candidate);
    }

    if !summary.has_accel_candidate {
        assert!(!summary.should_crosscheck);
        assert!(!summary.should_experimental_accept);
    }

    if summary.should_experimental_accept {
        assert!(summary.should_crosscheck || cfg!(feature = "halo2-accel-verify"));
        assert_eq!(config.mode, Halo2AccelMode::ExperimentalAccept);
    }
});

fn accel_config_from_bytes(data: &[u8]) -> Halo2AccelConfig {
    Halo2AccelConfig {
        enabled: byte_at(data, 1) & 1 == 1,
        backend: match byte_at(data, 2) % 3 {
            0 => Halo2AccelBackend::Auto,
            1 => Halo2AccelBackend::Cuda,
            _ => Halo2AccelBackend::Avx512,
        },
        mode: match byte_at(data, 3) % 3 {
            0 => Halo2AccelMode::Cpu,
            1 => Halo2AccelMode::Crosscheck,
            _ => Halo2AccelMode::ExperimentalAccept,
        },
        min_batch_actions: usize::from(byte_at(data, 4) % 16),
        min_msm_size: 1usize << u32::from(byte_at(data, 5) % 16),
    }
}

fn byte_at(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or_default()
}

fn extract_halo2_items_from_blocks() -> Vec<Item> {
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
            items.push(Item::new(bundle, sighash));
        }
    }

    assert!(
        !items.is_empty(),
        "NU5+ test blocks must contain Orchard transactions without transparent inputs"
    );

    items
}
