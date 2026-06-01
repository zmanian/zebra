//! Async Halo2 batch verifier service

use std::{
    fmt,
    future::Future,
    mem,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use orchard::{bundle::BatchValidator, circuit::VerifyingKey};
use rand::thread_rng;
use zcash_protocol::value::ZatBalance;
use zebra_chain::transaction::SigHash;

use crate::{
    config::{Halo2AccelConfig, Halo2AccelMode},
    BoxError,
};
use thiserror::Error;
use tokio::sync::watch;
use tower::{util::ServiceFn, Service};
use tower_batch_control::{Batch, BatchControl, RequestWeight};
use tower_fallback::Fallback;

use super::spawn_fifo;

/// Adjusted batch size for halo2 batches.
///
/// Unlike other batch verifiers, halo2 has aggregate proofs.
/// This means that there can be hundreds of actions verified by some proofs,
/// but just one action in others.
///
/// To compensate for larger proofs, we process the batch once there are over
/// [`HALO2_MAX_BATCH_SIZE`] total actions among pending items in the queue.
const HALO2_MAX_BATCH_SIZE: usize = super::MAX_BATCH_SIZE;

/// The type of verification results.
type VerifyResult = bool;

/// The type of the batch sender channel.
type Sender = watch::Sender<Option<VerifyResult>>;

/// Temporary substitute type for fake batch verification.
///
/// TODO: implement batch verification
pub type BatchVerifyingKey = ItemVerifyingKey;

/// The type of a prepared verifying key.
/// This is the key used to verify individual items.
pub type ItemVerifyingKey = VerifyingKey;

lazy_static::lazy_static! {
    /// The halo2 proof verifying key.
    pub static ref VERIFYING_KEY: ItemVerifyingKey = ItemVerifyingKey::build();
}

/// A Halo2 verification item, used as the request type of the service.
#[derive(Clone, Debug)]
pub struct Item {
    bundle: orchard::bundle::Bundle<orchard::bundle::Authorized, ZatBalance>,
    sighash: SigHash,
    halo2_accel_config: Halo2AccelConfig,
}

impl RequestWeight for Item {
    fn request_weight(&self) -> usize {
        self.bundle.actions().len()
    }
}

impl Item {
    /// Creates a new [`Item`] from a bundle and sighash.
    pub fn new(
        bundle: orchard::bundle::Bundle<orchard::bundle::Authorized, ZatBalance>,
        sighash: SigHash,
    ) -> Self {
        Self::new_with_accel_config(bundle, sighash, Halo2AccelConfig::default())
    }

    /// Creates a new [`Item`] from a bundle, sighash, and Halo2 acceleration config.
    pub fn new_with_accel_config(
        bundle: orchard::bundle::Bundle<orchard::bundle::Authorized, ZatBalance>,
        sighash: SigHash,
        halo2_accel_config: Halo2AccelConfig,
    ) -> Self {
        Self {
            bundle,
            sighash,
            halo2_accel_config,
        }
    }

    /// Perform non-batched verification of this [`Item`].
    ///
    /// This is useful (in combination with `Item::clone`) for implementing
    /// fallback logic when batch verification fails.
    pub fn verify_single(self, vk: &ItemVerifyingKey) -> bool {
        let mut batch = BatchValidator::default();
        batch.queue(self);
        batch.validate(vk, thread_rng())
    }
}

/// Acceleration metadata accumulated for one Halo2 verifier batch.
#[derive(Clone, Debug, Default)]
struct Halo2BatchAccelContext {
    batch_actions: usize,
    candidate_items: usize,
    config: Halo2AccelConfig,
}

/// Result metadata for one accelerated verifier batch attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Halo2VerifyOutcome {
    accepted: bool,
    crosscheck_mismatch: bool,
    accelerated_result: Option<bool>,
    cpu_result: Option<bool>,
    dispatch_stats: Halo2DispatchStats,
}

/// Acceleration facade stats captured for one verifier batch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Halo2DispatchStats {
    msm_candidate_points: u64,
    msm_fallbacks: u64,
}

impl Halo2BatchAccelContext {
    fn observe_item(&mut self, item: &Item) {
        self.observe_actions(item.request_weight(), &item.halo2_accel_config);
    }

    fn observe_actions(&mut self, action_count: usize, config: &Halo2AccelConfig) {
        self.batch_actions += action_count;
        if config.should_try_accel(action_count) {
            self.candidate_items += 1;
        }
        self.config = config.clone();
    }

    fn enabled_label(&self) -> &'static str {
        if self.config.enabled {
            "true"
        } else {
            "false"
        }
    }

    fn candidate_label(&self) -> &'static str {
        if self.candidate_items > 0 {
            "true"
        } else {
            "false"
        }
    }

    fn compiled_label(&self) -> &'static str {
        if cfg!(feature = "halo2-accel-verify") {
            "true"
        } else {
            "false"
        }
    }

    fn backend_label(&self) -> &'static str {
        self.config.backend.as_metric_label()
    }

    fn mode_label(&self) -> &'static str {
        self.config.mode.as_metric_label()
    }

    fn has_accel_candidate(&self) -> bool {
        self.candidate_items > 0
    }

    fn should_crosscheck(&self) -> bool {
        self.has_accel_candidate()
            && cfg!(feature = "halo2-accel-verify")
            && (self.config.mode == Halo2AccelMode::Crosscheck
                || (self.config.mode == Halo2AccelMode::ExperimentalAccept
                    && !self.should_experimental_accept()))
    }

    fn should_experimental_accept(&self) -> bool {
        self.has_accel_candidate()
            && cfg!(feature = "halo2-accel-verify")
            && cfg!(feature = "experimental-verifier-accept")
            && self.config.mode == Halo2AccelMode::ExperimentalAccept
            && self.accelerated_backend_self_test_passed()
    }

    #[cfg(feature = "halo2-accel-verify")]
    fn requested_backend(&self) -> zcash_pasta_accel::Backend {
        match self.config.backend {
            crate::config::Halo2AccelBackend::Auto => zcash_pasta_accel::Backend::Auto,
            crate::config::Halo2AccelBackend::Cuda => zcash_pasta_accel::Backend::Cuda,
            crate::config::Halo2AccelBackend::Avx512 => zcash_pasta_accel::Backend::Avx512,
        }
    }

    fn accelerated_backend_self_test_passed(&self) -> bool {
        #[cfg(feature = "halo2-accel-verify")]
        {
            self.has_accel_candidate()
                && zcash_pasta_accel::accelerated_backend_self_test(self.requested_backend())
        }

        #[cfg(not(feature = "halo2-accel-verify"))]
        {
            false
        }
    }

    #[cfg(feature = "halo2-accel-verify")]
    fn dispatch_config(&self) -> zcash_pasta_accel::DispatchConfig {
        let backend = if self.has_accel_candidate() && self.config.mode != Halo2AccelMode::Cpu {
            self.requested_backend()
        } else {
            zcash_pasta_accel::Backend::Cpu
        };

        zcash_pasta_accel::DispatchConfig {
            backend,
            min_msm_size: self.config.min_msm_size,
        }
    }

    #[cfg(feature = "halo2-accel-verify")]
    fn cpu_dispatch_config(&self) -> zcash_pasta_accel::DispatchConfig {
        zcash_pasta_accel::DispatchConfig {
            backend: zcash_pasta_accel::Backend::Cpu,
            min_msm_size: self.config.min_msm_size,
        }
    }

    fn resolve_result_without_crosscheck(&self, accelerated_result: bool) -> Halo2VerifyOutcome {
        Halo2VerifyOutcome {
            accepted: accelerated_result,
            crosscheck_mismatch: false,
            accelerated_result: Some(accelerated_result),
            cpu_result: None,
            dispatch_stats: Halo2DispatchStats::default(),
        }
    }

    fn resolve_result_after_crosscheck(
        &self,
        accelerated_result: bool,
        cpu_result: bool,
    ) -> Halo2VerifyOutcome {
        let crosscheck_mismatch = accelerated_result != cpu_result;
        let accepted = if self.should_experimental_accept() {
            accelerated_result
        } else {
            cpu_result
        };

        Halo2VerifyOutcome {
            accepted,
            crosscheck_mismatch,
            accelerated_result: Some(accelerated_result),
            cpu_result: Some(cpu_result),
            dispatch_stats: Halo2DispatchStats::default(),
        }
    }

    fn record_selection_metrics(&self) {
        metrics::gauge!(
            "zebra.consensus.halo2.accel.backend",
            "backend" => self.backend_label(),
            "candidate" => self.candidate_label(),
            "compiled" => self.compiled_label(),
            "enabled" => self.enabled_label(),
            "mode" => self.mode_label(),
        )
        .set(1.0);

        metrics::gauge!(
            "zebra.consensus.halo2.accel.mode",
            "backend" => self.backend_label(),
            "candidate" => self.candidate_label(),
            "compiled" => self.compiled_label(),
            "enabled" => self.enabled_label(),
            "mode" => self.mode_label(),
        )
        .set(1.0);
    }

    fn record_flush_metrics(&self, duration: f64, result_label: &'static str) {
        self.record_selection_metrics();

        metrics::histogram!(
            "zebra.consensus.halo2.accel.batch_actions",
            "backend" => self.backend_label(),
            "candidate" => self.candidate_label(),
            "compiled" => self.compiled_label(),
            "enabled" => self.enabled_label(),
            "mode" => self.mode_label(),
        )
        .record(self.batch_actions as f64);

        metrics::histogram!(
            "zebra.consensus.halo2.accel.duration_seconds",
            "backend" => self.backend_label(),
            "candidate" => self.candidate_label(),
            "compiled" => self.compiled_label(),
            "enabled" => self.enabled_label(),
            "mode" => self.mode_label(),
            "result" => result_label,
        )
        .record(duration);
    }

    fn record_crosscheck_metrics(&self, outcome: &Halo2VerifyOutcome) {
        if outcome.cpu_result.is_none() {
            return;
        }

        let mismatch_label = if outcome.crosscheck_mismatch {
            "true"
        } else {
            "false"
        };

        metrics::counter!(
            "zebra.consensus.halo2.accel.crosscheck_mismatches",
            "backend" => self.backend_label(),
            "compiled" => self.compiled_label(),
            "enabled" => self.enabled_label(),
            "mismatch" => mismatch_label,
            "mode" => self.mode_label(),
        )
        .increment(usize::from(outcome.crosscheck_mismatch) as u64);
    }

    fn record_dispatch_stats(&self, stats: Halo2DispatchStats) {
        if !self.has_accel_candidate() {
            return;
        }

        if stats.msm_candidate_points > 0 {
            metrics::histogram!(
                "zebra.consensus.halo2.accel.msm_points",
                "backend" => self.backend_label(),
                "compiled" => self.compiled_label(),
                "enabled" => self.enabled_label(),
                "mode" => self.mode_label(),
            )
            .record(stats.msm_candidate_points as f64);
        }

        if stats.msm_fallbacks > 0 {
            metrics::counter!(
                "zebra.consensus.halo2.accel.fallbacks",
                "backend" => self.backend_label(),
                "compiled" => self.compiled_label(),
                "enabled" => self.enabled_label(),
                "mode" => self.mode_label(),
            )
            .increment(stats.msm_fallbacks);
        }
    }
}

#[cfg(feature = "fuzz-impl")]
pub mod fuzz {
    //! Fuzz-only helpers for exercising Halo2 batch-item selection logic.

    use crate::config::Halo2AccelConfig;

    use super::{Halo2BatchAccelContext, Item};

    /// Summary of the acceleration metadata derived from a batch of Halo2 items.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct BatchItemSummary {
        /// Total Orchard actions represented by the batch items.
        pub batch_actions: usize,

        /// Number of items that meet the acceleration candidate threshold.
        pub candidate_items: usize,

        /// Whether any item in the batch is an acceleration candidate.
        pub has_accel_candidate: bool,

        /// Whether Zebra would run CPU crosscheck protection for this batch.
        pub should_crosscheck: bool,

        /// Whether this build and backend can accept accelerated results directly.
        pub should_experimental_accept: bool,
    }

    /// Clone a real Halo2 item while replacing its acceleration configuration.
    pub fn clone_item_with_accel_config(item: &Item, halo2_accel_config: Halo2AccelConfig) -> Item {
        Item {
            bundle: item.bundle.clone(),
            sighash: item.sighash.clone(),
            halo2_accel_config,
        }
    }

    /// Summarize the batch acceleration decisions Zebra derives from real Halo2 items.
    pub fn summarize_batch_items(items: &[Item]) -> BatchItemSummary {
        let mut context = Halo2BatchAccelContext::default();

        for item in items {
            context.observe_item(item);
        }

        BatchItemSummary {
            batch_actions: context.batch_actions,
            candidate_items: context.candidate_items,
            has_accel_candidate: context.has_accel_candidate(),
            should_crosscheck: context.should_crosscheck(),
            should_experimental_accept: context.should_experimental_accept(),
        }
    }
}

trait QueueBatchVerify {
    fn queue(&mut self, item: Item);
}

impl QueueBatchVerify for BatchValidator {
    fn queue(
        &mut self,
        Item {
            bundle, sighash, ..
        }: Item,
    ) {
        self.add_bundle(&bundle, sighash.0);
    }
}

/// An error that may occur when verifying [Halo2 proofs of Zcash Orchard Action
/// descriptions][actions].
///
/// [actions]: https://zips.z.cash/protocol/protocol.pdf#actiondesc
// TODO: if halo2::plonk::Error gets the std::error::Error trait derived on it,
// remove this and just wrap `halo2::plonk::Error` as an enum variant of
// `crate::transaction::Error`, which does the trait derivation via `thiserror`
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[allow(missing_docs)]
pub enum Halo2Error {
    #[error("the constraint system is not satisfied")]
    ConstraintSystemFailure,
    #[error("unknown Halo2 error")]
    Other,
}

impl From<halo2::plonk::Error> for Halo2Error {
    fn from(err: halo2::plonk::Error) -> Halo2Error {
        match err {
            halo2::plonk::Error::ConstraintSystemFailure => Halo2Error::ConstraintSystemFailure,
            _ => Halo2Error::Other,
        }
    }
}

/// Global batch verification context for Halo2 proofs of Action statements.
///
/// This service transparently batches contemporaneous proof verifications,
/// handling batch failures by falling back to individual verification.
///
/// Note that making a `Service` call requires mutable access to the service, so
/// you should call `.clone()` on the global handle to create a local, mutable
/// handle.
pub static VERIFIER: Lazy<
    Fallback<
        Batch<Verifier, Item>,
        ServiceFn<fn(Item) -> BoxFuture<'static, Result<(), BoxError>>>,
    >,
> = Lazy::new(|| {
    Fallback::new(
        Batch::new(
            Verifier::new(&VERIFYING_KEY),
            HALO2_MAX_BATCH_SIZE,
            None,
            super::MAX_BATCH_LATENCY,
        ),
        // We want to fallback to individual verification if batch verification fails,
        // so we need a Service to use.
        //
        // Because we have to specify the type of a static, we need to be able to
        // write the type of the closure and its return value. But both closures and
        // async blocks have unnameable types. So instead we cast the closure to a function
        // (which is possible because it doesn't capture any state), and use a BoxFuture
        // to erase the result type.
        // (We can't use BoxCloneService to erase the service type, because it is !Sync.)
        tower::service_fn(
            (|item: Item| Verifier::verify_single_spawning(item, &VERIFYING_KEY).boxed())
                as fn(_) -> _,
        ),
    )
});

/// Halo2 proof verifier implementation
///
/// This is the core implementation for the batch verification logic of the
/// Halo2 verifier. It handles batching incoming requests, driving batches to
/// completion, and reporting results.
pub struct Verifier {
    /// The synchronous Halo2 batch validator.
    batch: BatchValidator,

    /// A CPU-only copy of the batch for crosscheck mode.
    cpu_crosscheck_batch: BatchValidator,

    /// The halo2 proof verification key.
    ///
    /// Making this 'static makes managing lifetimes much easier.
    vk: &'static ItemVerifyingKey,

    /// A channel for broadcasting the result of a batch to the futures for each batch item.
    ///
    /// Each batch gets a newly created channel, so there is only ever one result sent per channel.
    /// Tokio doesn't have a oneshot multi-consumer channel, so we use a watch channel.
    tx: Sender,

    /// Acceleration metadata for the currently accumulating batch.
    halo2_accel_context: Halo2BatchAccelContext,
}

impl Verifier {
    fn new(vk: &'static ItemVerifyingKey) -> Self {
        let batch = BatchValidator::default();
        let (tx, _) = watch::channel(None);
        Self {
            batch,
            cpu_crosscheck_batch: BatchValidator::default(),
            vk,
            tx,
            halo2_accel_context: Halo2BatchAccelContext::default(),
        }
    }

    /// Returns the batch verifier and channel sender from `self`,
    /// replacing them with a new empty batch.
    fn take(
        &mut self,
    ) -> (
        BatchValidator,
        BatchValidator,
        &'static BatchVerifyingKey,
        Sender,
        Halo2BatchAccelContext,
    ) {
        // Use a new verifier and channel for each batch.
        let batch = mem::take(&mut self.batch);
        let cpu_crosscheck_batch = mem::take(&mut self.cpu_crosscheck_batch);
        let halo2_accel_context = mem::take(&mut self.halo2_accel_context);

        let (tx, _) = watch::channel(None);
        let tx = mem::replace(&mut self.tx, tx);

        (
            batch,
            cpu_crosscheck_batch,
            self.vk,
            tx,
            halo2_accel_context,
        )
    }

    /// Synchronously process the batch, and send the result using the channel sender.
    /// This function blocks until the batch is completed.
    fn verify(
        batch: BatchValidator,
        cpu_crosscheck_batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        tx: Sender,
        halo2_accel_context: Halo2BatchAccelContext,
    ) {
        let start = std::time::Instant::now();
        let outcome =
            Self::verify_batch_pair(batch, cpu_crosscheck_batch, vk, &halo2_accel_context);
        let duration = start.elapsed().as_secs_f64();
        let result_label = if outcome.accepted {
            "success"
        } else {
            "failure"
        };
        halo2_accel_context.record_flush_metrics(duration, result_label);
        halo2_accel_context.record_crosscheck_metrics(&outcome);
        halo2_accel_context.record_dispatch_stats(outcome.dispatch_stats);
        let _ = tx.send(Some(outcome.accepted));
    }

    /// Flush the batch using a thread pool, and return the result via the channel.
    /// This returns immediately, usually before the batch is completed.
    fn flush_blocking(&mut self) {
        let (batch, cpu_crosscheck_batch, vk, tx, halo2_accel_context) = self.take();

        // Correctness: Do CPU-intensive work on a dedicated thread, to avoid blocking other futures.
        //
        // We don't care about execution order here, because this method is only called on drop.
        tokio::task::block_in_place(|| {
            rayon::spawn_fifo(|| {
                Self::verify(batch, cpu_crosscheck_batch, vk, tx, halo2_accel_context)
            })
        });
    }

    /// Flush the batch using a thread pool, and return the result via the channel.
    /// This function returns a future that becomes ready when the batch is completed.
    async fn flush_spawning(
        batch: BatchValidator,
        cpu_crosscheck_batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        tx: Sender,
        halo2_accel_context: Halo2BatchAccelContext,
    ) {
        // Correctness: Do CPU-intensive work on a dedicated thread, to avoid blocking other futures.
        let start = std::time::Instant::now();
        let metrics_context = halo2_accel_context.clone();
        let result = spawn_fifo(move || {
            Self::verify_batch_pair(batch, cpu_crosscheck_batch, vk, &halo2_accel_context)
        })
        .await;
        let duration = start.elapsed().as_secs_f64();

        let result_label = match &result {
            Ok(outcome) if outcome.accepted => "success",
            _ => "failure",
        };
        metrics::histogram!(
            "zebra.consensus.batch.duration_seconds",
            "verifier" => "halo2",
            "result" => result_label
        )
        .record(duration);
        metrics_context.record_flush_metrics(duration, result_label);
        if let Ok(outcome) = &result {
            metrics_context.record_crosscheck_metrics(outcome);
            metrics_context.record_dispatch_stats(outcome.dispatch_stats);
        }

        let _ = tx.send(result.ok().map(|outcome| outcome.accepted));
    }

    fn verify_batch_pair(
        batch: BatchValidator,
        cpu_crosscheck_batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        halo2_accel_context: &Halo2BatchAccelContext,
    ) -> Halo2VerifyOutcome {
        let accelerated_result = Self::validate_accel_batch(batch, vk, halo2_accel_context);
        let dispatch_stats = Self::take_dispatch_stats();

        let mut outcome = if halo2_accel_context.should_crosscheck() {
            let cpu_result =
                Self::validate_cpu_batch(cpu_crosscheck_batch, vk, halo2_accel_context);
            halo2_accel_context.resolve_result_after_crosscheck(accelerated_result, cpu_result)
        } else {
            halo2_accel_context.resolve_result_without_crosscheck(accelerated_result)
        };

        outcome.dispatch_stats = dispatch_stats;
        outcome
    }

    #[cfg(feature = "halo2-accel-verify")]
    fn validate_accel_batch(
        batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        halo2_accel_context: &Halo2BatchAccelContext,
    ) -> bool {
        zcash_pasta_accel::reset_dispatch_stats();
        zcash_pasta_accel::with_dispatch_config(halo2_accel_context.dispatch_config(), || {
            batch.validate(vk, thread_rng())
        })
    }

    #[cfg(not(feature = "halo2-accel-verify"))]
    fn validate_accel_batch(
        batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        _halo2_accel_context: &Halo2BatchAccelContext,
    ) -> bool {
        batch.validate(vk, thread_rng())
    }

    #[cfg(feature = "halo2-accel-verify")]
    fn take_dispatch_stats() -> Halo2DispatchStats {
        let stats = zcash_pasta_accel::take_dispatch_stats();
        Halo2DispatchStats {
            msm_candidate_points: stats.msm_candidate_points,
            msm_fallbacks: stats.msm_fallbacks,
        }
    }

    #[cfg(not(feature = "halo2-accel-verify"))]
    fn take_dispatch_stats() -> Halo2DispatchStats {
        Halo2DispatchStats::default()
    }

    #[cfg(feature = "halo2-accel-verify")]
    fn validate_cpu_batch(
        batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        halo2_accel_context: &Halo2BatchAccelContext,
    ) -> bool {
        zcash_pasta_accel::reset_dispatch_stats();
        let result = zcash_pasta_accel::with_dispatch_config(
            halo2_accel_context.cpu_dispatch_config(),
            || batch.validate(vk, thread_rng()),
        );
        let _ = zcash_pasta_accel::take_dispatch_stats();
        result
    }

    #[cfg(not(feature = "halo2-accel-verify"))]
    fn validate_cpu_batch(
        batch: BatchValidator,
        vk: &'static BatchVerifyingKey,
        _halo2_accel_context: &Halo2BatchAccelContext,
    ) -> bool {
        batch.validate(vk, thread_rng())
    }

    /// Verify a single item using a thread pool, and return the result.
    async fn verify_single_spawning(
        item: Item,
        pvk: &'static ItemVerifyingKey,
    ) -> Result<(), BoxError> {
        // TODO: Restore code for verifying single proofs or return a result from batch.validate()
        // Correctness: Do CPU-intensive work on a dedicated thread, to avoid blocking other futures.
        if spawn_fifo(move || item.verify_single(pvk)).await? {
            Ok(())
        } else {
            Err("could not validate orchard proof".into())
        }
    }
}

impl fmt::Debug for Verifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = "Verifier";
        f.debug_struct(name)
            .field("batch", &"..")
            .field("vk", &"..")
            .field("tx", &self.tx)
            .finish()
    }
}

impl Service<BatchControl<Item>> for Verifier {
    type Response = ();
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send + 'static>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: BatchControl<Item>) -> Self::Future {
        match req {
            BatchControl::Item(item) => {
                tracing::trace!("got item");
                self.halo2_accel_context.observe_item(&item);
                self.cpu_crosscheck_batch.queue(item.clone());
                self.batch.queue(item);
                let mut rx = self.tx.subscribe();
                Box::pin(async move {
                    match rx.changed().await {
                        Ok(()) => {
                            // We use a new channel for each batch,
                            // so we always get the correct batch result here.
                            let is_valid = *rx
                                .borrow()
                                .as_ref()
                                .ok_or("threadpool unexpectedly dropped response channel sender. Is Zebra shutting down?")?;

                            if is_valid {
                                tracing::trace!(?is_valid, "verified halo2 proof");
                                metrics::counter!("proofs.halo2.verified").increment(1);
                                Ok(())
                            } else {
                                tracing::trace!(?is_valid, "invalid halo2 proof");
                                metrics::counter!("proofs.halo2.invalid").increment(1);
                                Err("could not validate halo2 proofs".into())
                            }
                        }
                        Err(_recv_error) => panic!("verifier was dropped without flushing"),
                    }
                })
            }

            BatchControl::Flush => {
                tracing::trace!("got halo2 flush command");

                let (batch, cpu_crosscheck_batch, vk, tx, halo2_accel_context) = self.take();

                Box::pin(
                    Self::flush_spawning(batch, cpu_crosscheck_batch, vk, tx, halo2_accel_context)
                        .map(Ok),
                )
            }
        }
    }
}

impl Drop for Verifier {
    fn drop(&mut self) {
        // We need to flush the current batch in case there are still any pending futures.
        // This returns immediately, usually before the batch is completed.
        self.flush_blocking()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Halo2AccelBackend, Halo2AccelConfig, Halo2AccelMode};

    use super::Halo2BatchAccelContext;
    use metrics::{
        Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, KeyName, Metadata,
        Recorder, SharedString, Unit,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq)]
    struct RecordedGauge {
        name: String,
        labels: Vec<(String, String)>,
        value: f64,
    }

    #[derive(Clone, Default)]
    struct GaugeRecorder {
        gauges: Arc<Mutex<Vec<RecordedGauge>>>,
    }

    impl GaugeRecorder {
        fn gauges(&self) -> Vec<RecordedGauge> {
            self.gauges.lock().expect("recorder lock succeeds").clone()
        }
    }

    struct RecordingGauge {
        name: String,
        labels: Vec<(String, String)>,
        gauges: Arc<Mutex<Vec<RecordedGauge>>>,
    }

    impl GaugeFn for RecordingGauge {
        fn increment(&self, _value: f64) {}

        fn decrement(&self, _value: f64) {}

        fn set(&self, value: f64) {
            self.gauges
                .lock()
                .expect("recorder lock succeeds")
                .push(RecordedGauge {
                    name: self.name.clone(),
                    labels: self.labels.clone(),
                    value,
                });
        }
    }

    impl CounterFn for RecordingGauge {
        fn increment(&self, _value: u64) {}

        fn absolute(&self, _value: u64) {}
    }

    impl HistogramFn for RecordingGauge {
        fn record(&self, _value: f64) {}
    }

    impl Recorder for GaugeRecorder {
        fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {
        }

        fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

        fn describe_histogram(
            &self,
            _key: KeyName,
            _unit: Option<Unit>,
            _description: SharedString,
        ) {
        }

        fn register_counter(&self, _key: &Key, _metadata: &Metadata<'_>) -> Counter {
            Counter::noop()
        }

        fn register_gauge(&self, key: &Key, _metadata: &Metadata<'_>) -> Gauge {
            let gauge = RecordingGauge {
                name: key.name().to_string(),
                labels: key
                    .labels()
                    .map(|label| (label.key().to_string(), label.value().to_string()))
                    .collect(),
                gauges: self.gauges.clone(),
            };

            Gauge::from_arc(Arc::new(gauge))
        }

        fn register_histogram(&self, _key: &Key, _metadata: &Metadata<'_>) -> Histogram {
            Histogram::noop()
        }
    }

    #[test]
    fn halo2_batch_accel_context_tracks_candidate_actions() {
        let mut config = Halo2AccelConfig {
            enabled: true,
            backend: Halo2AccelBackend::Cuda,
            mode: Halo2AccelMode::Crosscheck,
            min_batch_actions: 2,
            min_msm_size: 4096,
        };
        let mut context = Halo2BatchAccelContext::default();

        context.observe_actions(1, &config);
        assert_eq!(context.batch_actions, 1);
        assert_eq!(context.candidate_items, 0);
        assert_eq!(context.candidate_label(), "false");

        context.observe_actions(3, &config);
        assert_eq!(context.batch_actions, 4);
        assert_eq!(context.candidate_items, 1);
        assert_eq!(context.candidate_label(), "true");
        assert_eq!(context.backend_label(), "cuda");
        assert_eq!(context.mode_label(), "crosscheck");

        config.enabled = false;
        context.observe_actions(10, &config);
        assert_eq!(context.batch_actions, 14);
        assert_eq!(context.candidate_items, 1);
        assert_eq!(context.enabled_label(), "false");
    }

    #[test]
    fn halo2_batch_accel_records_backend_and_mode_selection_metrics() {
        let config = Halo2AccelConfig {
            enabled: true,
            backend: Halo2AccelBackend::Avx512,
            mode: Halo2AccelMode::Crosscheck,
            min_batch_actions: 2,
            min_msm_size: 4096,
        };
        let mut context = Halo2BatchAccelContext::default();
        context.observe_actions(3, &config);

        let recorder = GaugeRecorder::default();
        metrics::with_local_recorder(&recorder, || context.record_selection_metrics());

        let gauges = recorder.gauges();
        assert!(gauges.iter().any(|gauge| {
            gauge.name == "zebra.consensus.halo2.accel.backend"
                && gauge.value == 1.0
                && gauge
                    .labels
                    .iter()
                    .any(|(key, value)| key == "backend" && value == "avx512")
                && gauge
                    .labels
                    .iter()
                    .any(|(key, value)| key == "mode" && value == "crosscheck")
        }));
        assert!(gauges.iter().any(|gauge| {
            gauge.name == "zebra.consensus.halo2.accel.mode"
                && gauge.value == 1.0
                && gauge
                    .labels
                    .iter()
                    .any(|(key, value)| key == "backend" && value == "avx512")
                && gauge
                    .labels
                    .iter()
                    .any(|(key, value)| key == "mode" && value == "crosscheck")
        }));
    }

    #[cfg(feature = "halo2-accel-verify")]
    #[test]
    fn halo2_batch_accel_context_uses_scoped_dispatch_and_cpu_crosscheck_acceptance() {
        let config = Halo2AccelConfig {
            enabled: true,
            backend: Halo2AccelBackend::Cuda,
            mode: Halo2AccelMode::Crosscheck,
            min_batch_actions: 2,
            min_msm_size: 8192,
        };
        let mut context = Halo2BatchAccelContext::default();

        context.observe_actions(3, &config);

        assert!(context.should_crosscheck());
        assert_eq!(
            context.dispatch_config(),
            zcash_pasta_accel::DispatchConfig {
                backend: zcash_pasta_accel::Backend::Cuda,
                min_msm_size: 8192,
            }
        );

        let outcome = context.resolve_result_after_crosscheck(false, true);
        assert!(outcome.accepted);
        assert!(outcome.crosscheck_mismatch);
        assert_eq!(outcome.accelerated_result, Some(false));
        assert_eq!(outcome.cpu_result, Some(true));
    }

    #[cfg(feature = "halo2-accel-verify")]
    #[test]
    fn halo2_batch_accel_takes_facade_dispatch_stats() {
        use super::{Halo2DispatchStats, Verifier};

        zcash_pasta_accel::reset_dispatch_stats();
        zcash_pasta_accel::record_msm_candidate(7);
        zcash_pasta_accel::record_msm_candidate(11);
        zcash_pasta_accel::record_msm_fallback();

        assert_eq!(
            Verifier::take_dispatch_stats(),
            Halo2DispatchStats {
                msm_candidate_points: 18,
                msm_fallbacks: 1,
            }
        );
        assert_eq!(
            Verifier::take_dispatch_stats(),
            Halo2DispatchStats::default()
        );
    }

    #[cfg(feature = "halo2-accel-verify")]
    #[test]
    fn halo2_batch_accel_experimental_accept_requires_build_feature() {
        let config = Halo2AccelConfig {
            enabled: true,
            backend: Halo2AccelBackend::Cuda,
            mode: Halo2AccelMode::ExperimentalAccept,
            min_batch_actions: 2,
            min_msm_size: 8192,
        };
        let mut context = Halo2BatchAccelContext::default();

        context.observe_actions(3, &config);

        if cfg!(feature = "experimental-verifier-accept")
            && zcash_pasta_accel::accelerated_backend_self_test(zcash_pasta_accel::Backend::Cuda)
        {
            assert!(context.should_experimental_accept());
            assert!(!context.should_crosscheck());
            assert!(context.resolve_result_without_crosscheck(true).accepted);
        } else {
            assert!(!context.should_experimental_accept());
            assert!(context.should_crosscheck());
            let outcome = context.resolve_result_after_crosscheck(false, true);
            assert!(outcome.accepted);
            assert!(outcome.crosscheck_mismatch);
        }
    }

    #[cfg(all(
        feature = "halo2-accel-verify",
        feature = "experimental-verifier-accept"
    ))]
    #[test]
    fn halo2_batch_accel_experimental_accept_requires_backend_self_test() {
        let config = Halo2AccelConfig {
            enabled: true,
            backend: Halo2AccelBackend::Cuda,
            mode: Halo2AccelMode::ExperimentalAccept,
            min_batch_actions: 2,
            min_msm_size: 8192,
        };
        let mut context = Halo2BatchAccelContext::default();

        context.observe_actions(3, &config);

        if !zcash_pasta_accel::accelerated_backend_self_test(zcash_pasta_accel::Backend::Cuda) {
            assert!(!context.should_experimental_accept());
            assert!(context.should_crosscheck());
            let outcome = context.resolve_result_after_crosscheck(false, true);
            assert!(outcome.accepted);
            assert!(outcome.crosscheck_mismatch);
        }
    }
}
