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
#[cfg(feature = "halo2-accel-verify")]
use zebra_consensus::config::{Halo2AccelBackend, Halo2AccelConfig, Halo2AccelMode};
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
    let vk = &*VERIFYING_KEY;
    let source_items = common::extract_halo2_items_from_blocks();
    #[cfg(feature = "halo2-accel-verify")]
    let crosscheck_source_items =
        common::extract_halo2_items_from_blocks_with_accel_config(Halo2AccelConfig {
            enabled: true,
            backend: Halo2AccelBackend::Auto,
            mode: Halo2AccelMode::Crosscheck,
            min_batch_actions: 1,
            min_msm_size: 1,
        });
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

        #[cfg(feature = "halo2-accel-verify")]
        {
            let crosscheck_items = common::cycled(&crosscheck_source_items, bundle_count);
            group.throughput(Throughput::Elements(total_actions(&crosscheck_items)));

            group.bench_with_input(
                BenchmarkId::new("crosscheck_batch_service", bundle_count),
                &crosscheck_items,
                |b, items| {
                    b.iter(|| {
                        runtime.block_on(verify_with_batch_service(items));
                    })
                },
            );
        }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().noise_threshold(0.1).sample_size(10);
    targets = bench_halo2_sandblast_replay
}
criterion_main!(benches);
