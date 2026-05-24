# Queued Block Height Index Desync Note

Date: 2026-05-03

Last updated: 2026-05-09

Disposition: local-only. Do not post publicly without explicit
re-authorization.

## Summary

`QueuedBlocks::dequeue_children()` removes the entire `by_height` bucket for
each dequeued child's height. If the queue contains multiple semantically
verified missing-parent blocks at the same height but under different parents,
dequeueing one parent's children removes the height index for the other queued
blocks without removing those blocks from `blocks` or `by_parent`.

After that index desync, `QueuedBlocks::prune_by_height()` can no longer find
the surviving same-height blocks by finalized height. Those blocks can remain
queued until their specific parents arrive, the state service is dropped, or a
separate cleanup path clears the whole queue. If the finalized tip later reaches
or passes the surviving child's height, its missing parent is below the
finalized tip and can no longer become a non-finalized parent, so the retained
child can become effectively permanent for that node process.

This is a public availability-hardening issue, not consensus-invalid block
acceptance. On Mainnet/Testnet, a remote block must already pass semantic
verification, including proof of work, before it can enter this queue.

## Evidence

Queue insertion indexes each block independently by hash, parent hash, and
height:

- `zebra-state/src/service/queued_blocks.rs:54-80`

`dequeue_children()` selects children by parent and removes the selected block
hashes from the primary `blocks` map:

- `zebra-state/src/service/queued_blocks.rs:93-107`

But for each dequeued child it calls:

- `zebra-state/src/service/queued_blocks.rs:109-110`

That removes the whole height bucket. It does not remove only the dequeued
child hash from the height bucket. If another queued block at that same height
is waiting on a different parent, it remains in:

- `blocks`, so it still retains the block and response sender,
- `by_parent`, so it can still be released if its parent arrives,
- `known_utxos`, except for best-effort removals that may also lose shared
  same-height data,
- but not `by_height`, so height pruning cannot find it.

Height pruning depends entirely on the `by_height` index:

- `zebra-state/src/service/queued_blocks.rs:131-145`

After collecting hashes from `by_height`, pruning removes them from `blocks` and
`by_parent`:

- `zebra-state/src/service/queued_blocks.rs:143-180`

The state service calls dequeue and pruning in the non-finalized commit path:

- `zebra-state/src/service.rs:746-760`
- `zebra-state/src/service.rs:787-820`

The existing tests cover same-height children of the same parent and pruning of
same-height siblings:

- `zebra-state/src/service/queued_blocks/tests/vectors.rs:30-96`
- `zebra-state/src/service/queued_blocks/tests/vectors.rs:99-139`

They do not cover same-height queued blocks under different missing parents,
which is the shape that desynchronizes the height index.

## Local Reproducer

Added durable current-behavior test
`dequeue_drops_height_index_for_other_parents_today` in
`zebra-state/src/service/queued_blocks/tests/vectors.rs`.

The original desired-behavior reproducer was:

```rust
#[test]
fn dequeue_preserves_height_index_for_other_parents() -> Result<()> {
    let _init_guard = zebra_test::init();

    let block1: Arc<Block> =
        zebra_test::vectors::BLOCK_MAINNET_419200_BYTES.zcash_deserialize_into()?;
    let parent1 = block1.clone().set_work(1);
    let parent2 = block1.set_work(2);
    let child1 = parent1.make_fake_child();
    let child2 = parent2.make_fake_child();
    let child_height = child1.coinbase_height().unwrap();

    assert_eq!(child_height, child2.coinbase_height().unwrap());
    assert_ne!(parent1.hash(), parent2.hash());

    let mut queue = QueuedBlocks::default();
    queue.queue(child1.clone().into_queued());
    queue.queue(child2.clone().into_queued());

    let children = queue.dequeue_children(parent1.hash());

    assert_eq!(1, children.len());
    assert_eq!(child1.hash(), children[0].0.hash);
    assert!(queue.get_mut(&child2.hash()).is_some());

    queue.prune_by_height(child_height);

    assert!(queue.get_mut(&child2.hash()).is_none());

    Ok(())
}
```

Command:

```text
cargo test -p zebra-state dequeue_preserves_height_index_for_other_parents --lib
```

Result:

```text
test service::queued_blocks::tests::vectors::dequeue_preserves_height_index_for_other_parents ... FAILED
assertion failed: queue.get_mut(&child2.hash()).is_none()
```

The failure confirms that after one same-height child is dequeued, pruning at
that height does not remove the remaining same-height child under another
parent.

The durable current-behavior test asserts the same bug in passing form:

- queue two same-height children under different fake parents;
- dequeue children for one parent;
- confirm the other child remains in `blocks`;
- confirm the entire `by_height[child_height]` bucket is gone;
- call `prune_by_height(child_height)`;
- confirm the remaining same-height child is still retained.

Verification rerun on 2026-05-09:

```text
cargo test -p zebra-state dequeue_drops_height_index_for_other_parents_today --lib
```

Result:

```text
test service::queued_blocks::tests::vectors::dequeue_drops_height_index_for_other_parents_today ... ok
```

Fresh rerun in this continuation:

```text
cargo test -p zebra-state dequeue_drops_height_index_for_other_parents_today --lib
```

Result: passed, 1 test.

The broader queued-block vector module was previously run on 2026-05-07.

Release reachability check: local `v4.4.1` and current `main` both have the
same `dequeue_children()` behavior: selected children are removed from
`blocks`, then the whole `by_height` bucket for each dequeued child's height is
removed. `prune_by_height()` in both versions still relies on `by_height` as
the source of hashes to expire.

## Duplicate Check

Read-only GitHub searches on 2026-05-09:

```sh
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra QueuedBlocks dequeue_children by_height prune_by_height'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "queued block" "height index" "prune"'
gh api -X GET search/issues -f q='repo:ZcashFoundation/zebra "dequeue_children" "by_height"'
```

Results:

- The direct `QueuedBlocks dequeue_children by_height prune_by_height` search
  returned no hits.
- The exact `dequeue_children` plus `by_height` search returned no hits.
- The broader quoted `queued block` / `height index` / `prune` search returned
  merged PR #902, a 2020 state-updates RFC, not a bug report or implementation
  fix for this secondary-index invariant.

## Impact

An attacker who can feed semantically verified missing-parent blocks at the same
height under different parents can make some queued blocks invisible to
finalized-height pruning. This strengthens the existing queued-block retention
lead because the retention can survive the normal pruning trigger, not just a
caller timeout. Once the finalized chain passes the missing parent's height,
the normal "parent eventually arrives" cleanup route is no longer realistic for
that queued child.

Existing practical bounds still matter:

- blocks must be semantically verified before queueing,
- proof of work is required on default public networks,
- inbound and sync block verification concurrency is bounded,
- queued blocks can still be released if their missing parents arrive,
- queue state is cleared when the state service is dropped.

So the likely severity is low-to-medium availability hardening for public
networks, sharper for custom/test networks where proof-of-work is disabled or
trusted feeders can submit semantically verified blocks cheaply.

## Suggested Fix

Change `dequeue_children()` to remove only the dequeued hash from the height
bucket:

```rust
if let Some(height_hashes) = self.by_height.get_mut(&queued.0.height) {
    height_hashes.remove(&queued.0.hash);
    if height_hashes.is_empty() {
        self.by_height.remove(&queued.0.height);
    }
}
```

Add the local reproducer as a regression test. The test should pass after the
height bucket update is fixed.

## Disclosure Triage

Public hardening.

This is not currently private-disclosure-worthy on its own because it does not
accept invalid blocks, reject valid blocks, or bypass proof-of-work on default
networks. Keep it in the local ledger unless explicitly re-authorized.

## Confidence

High for the secondary-index desync. Medium for operational impact, because
default exploitability depends on providing semantically valid proof-of-work
blocks with missing parents.
