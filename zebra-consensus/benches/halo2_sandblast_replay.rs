//! Offline replay benchmark for Orchard/Halo2 verification pressure.
//!
//! This benchmark is defensive: it replays local Zebra test vectors through the
//! existing Halo2 verifier paths and never sends traffic to public nodes.
//! Repeated bundles are used to approximate backlog pressure until historical
//! Sandblasting-era block data or a richer local corpus is available.

// Disabled due to warnings in criterion macros
#![allow(missing_docs)]

mod common;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use futures::{stream::FuturesUnordered, StreamExt};
use tokio::runtime::Runtime;
use tower::ServiceExt;
use tower_batch_control::RequestWeight;
use zebra_consensus::halo2::{self, Item, VERIFYING_KEY};

const REPLAY_BUNDLE_COUNTS: &[usize] = &[64, 128, 256];

async fn verify_with_batch_service(items: &[Item]) {
    let mut checks = FuturesUnordered::new();

    for item in items.iter().cloned() {
        checks.push(halo2::VERIFIER.clone().oneshot(item));
    }

    while let Some(result) = checks.next().await {
        result.expect("valid replay item verifies");
    }
}

fn total_actions(items: &[Item]) -> u64 {
    items.iter().map(|item| item.request_weight() as u64).sum()
}

fn bench_halo2_sandblast_replay(c: &mut Criterion) {
    // Keep the initial replay mode pinned to the current CPU accept path. Later
    // crosscheck and experimental-accept series should be added only when Zebra
    // has runtime support for those modes.
    std::env::set_var("ZCASH_ACCEL", "off");
    std::env::set_var("ZCASH_ACCEL_VERIFY_MODE", "cpu");

    let vk = &*VERIFYING_KEY;
    let source_items = common::extract_halo2_items_from_blocks();
    let runtime = Runtime::new().expect("tokio runtime builds");

    let mut group = c.benchmark_group("halo2_sandblast_replay");

    for &bundle_count in REPLAY_BUNDLE_COUNTS {
        let items = common::cycled(&source_items, bundle_count);
        group.throughput(Throughput::Elements(total_actions(&items)));

        group.bench_with_input(
            BenchmarkId::new("cpu_unbatched", bundle_count),
            &items,
            |b, items| {
                b.iter(|| {
                    for item in items {
                        assert!(item.clone().verify_single(vk));
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("cpu_batch_service", bundle_count),
            &items,
            |b, items| {
                b.iter(|| {
                    runtime.block_on(verify_with_batch_service(items));
                })
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().noise_threshold(0.1).sample_size(10);
    targets = bench_halo2_sandblast_replay
}
criterion_main!(benches);
