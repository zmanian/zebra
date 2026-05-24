//! Types used in `getrawmempool` RPC method.

use std::collections::{HashMap, HashSet};

#[cfg(test)]
use std::cell::Cell;

use derive_getters::Getters;
use derive_new::new;
use hex::ToHex as _;

use zebra_chain::{amount::NonNegative, block::Height, transaction::VerifiedUnminedTx};
use zebra_node_services::mempool::TransactionDependencies;

use super::zec::Zec;

/// Response to a `getrawmempool` RPC request.
///
/// See the notes for the [`Rpc::get_raw_mempool` method].
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum GetRawMempoolResponse {
    /// The transaction IDs, as hex strings (verbose=0)
    TxIds(Vec<String>),
    /// A map of transaction IDs to mempool transaction details objects
    /// (verbose=1)
    Verbose(HashMap<String, MempoolObject>),
}

/// A mempool transaction details object as returned by `getrawmempool` in
/// verbose mode.
#[allow(clippy::too_many_arguments)]
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, Getters, new)]
pub struct MempoolObject {
    /// Transaction size in bytes.
    pub(crate) size: u64,
    /// Transaction fee in zatoshi.
    #[getter(copy)]
    pub(crate) fee: Zec<NonNegative>,
    /// Transaction fee with fee deltas used for mining priority.
    #[serde(rename = "modifiedfee")]
    #[getter(copy)]
    pub(crate) modified_fee: Zec<NonNegative>,
    /// Local time transaction entered pool in seconds since 1 Jan 1970 GMT
    pub(crate) time: i64,
    /// Block height when transaction entered pool.
    #[getter(copy)]
    pub(crate) height: Height,
    /// Number of in-mempool descendant transactions (including this one).
    pub(crate) descendantcount: u64,
    /// Size of in-mempool descendants (including this one).
    pub(crate) descendantsize: u64,
    /// Modified fees (see "modifiedfee" above) of in-mempool descendants
    /// (including this one).
    pub(crate) descendantfees: u64,
    /// Transaction IDs of unconfirmed transactions used as inputs for this
    /// transaction.
    pub(crate) depends: Vec<String>,
}

impl MempoolObject {
    pub(crate) fn from_verified_unmined_tx(
        unmined_tx: &VerifiedUnminedTx,
        transactions: &[VerifiedUnminedTx],
        transaction_dependencies: &TransactionDependencies,
    ) -> Self {
        // Map transactions by their txids to make lookups easier
        let transactions_by_id = transactions_by_id(transactions);

        // Get txids of this transaction's descendants (dependents)
        let empty_set = HashSet::new();
        let deps = transaction_dependencies
            .dependents()
            .get(&unmined_tx.transaction.id.mined_id())
            .unwrap_or(&empty_set);
        let deps_len = deps.len();

        // For each dependent: get the tx, then its size and fee; then sum them
        // up
        let (deps_size, deps_fees) = deps
            .iter()
            .filter_map(|id| transactions_by_id.get(id))
            .map(|unmined_tx| (unmined_tx.transaction.size, unmined_tx.miner_fee))
            .reduce(|(size1, fee1), (size2, fee2)| {
                (size1 + size2, (fee1 + fee2).unwrap_or_default())
            })
            .unwrap_or((0, Default::default()));

        // Create the MempoolObject from the information we have gathered
        let mempool_object = MempoolObject {
            size: unmined_tx.transaction.size as u64,
            fee: unmined_tx.miner_fee.into(),
            // Change this if we ever support fee deltas (prioritisetransaction call)
            modified_fee: unmined_tx.miner_fee.into(),
            time: unmined_tx
                .time
                .map(|time| time.timestamp())
                .unwrap_or_default(),
            height: unmined_tx.height.unwrap_or(Height(0)),
            // Note that the following three count this transaction itself
            descendantcount: deps_len as u64 + 1,
            descendantsize: (deps_size + unmined_tx.transaction.size) as u64,
            descendantfees: (deps_fees + unmined_tx.miner_fee)
                .unwrap_or_default()
                .into(),
            // Get dependencies as a txid vector
            depends: transaction_dependencies
                .dependencies()
                .get(&unmined_tx.transaction.id.mined_id())
                .cloned()
                .unwrap_or_else(HashSet::new)
                .iter()
                .map(|id| id.encode_hex())
                .collect(),
        };
        mempool_object
    }
}

fn transactions_by_id(
    transactions: &[VerifiedUnminedTx],
) -> HashMap<zebra_chain::transaction::Hash, &VerifiedUnminedTx> {
    #[cfg(test)]
    {
        TRANSACTION_LOOKUP_MAP_BUILDS.with(|builds| builds.set(builds.get() + 1));
        TRANSACTION_LOOKUP_MAP_INPUTS
            .with(|inputs| inputs.set(inputs.get().saturating_add(transactions.len())));
    }

    transactions
        .iter()
        .map(|unmined_tx| (unmined_tx.transaction.id.mined_id(), unmined_tx))
        .collect()
}

#[cfg(test)]
thread_local! {
    static TRANSACTION_LOOKUP_MAP_BUILDS: Cell<usize> = const { Cell::new(0) };
    static TRANSACTION_LOOKUP_MAP_INPUTS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn reset_transaction_lookup_map_counters() {
    TRANSACTION_LOOKUP_MAP_BUILDS.with(|builds| builds.set(0));
    TRANSACTION_LOOKUP_MAP_INPUTS.with(|inputs| inputs.set(0));
}

#[cfg(test)]
fn transaction_lookup_map_counters() -> (usize, usize) {
    let builds = TRANSACTION_LOOKUP_MAP_BUILDS.with(Cell::get);
    let inputs = TRANSACTION_LOOKUP_MAP_INPUTS.with(Cell::get);

    (builds, inputs)
}

#[cfg(test)]
mod tests {
    use zebra_chain::{parameters::Network, transparent::OutPoint};

    use super::*;

    #[test]
    fn mempool_object_counts_only_direct_dependents_today() {
        let transactions = Network::Mainnet
            .unmined_transactions_in_blocks(..)
            .filter(|tx| !tx.transaction.transaction.is_coinbase())
            .take(3)
            .collect::<Vec<_>>();

        assert_eq!(
            transactions.len(),
            3,
            "test vectors should provide at least three non-coinbase mempool transactions"
        );

        let parent_id = transactions[0].transaction.id.mined_id();
        let child_id = transactions[1].transaction.id.mined_id();
        let grandchild_id = transactions[2].transaction.id.mined_id();

        let mut transaction_dependencies = TransactionDependencies::default();
        transaction_dependencies.add(
            child_id,
            vec![OutPoint {
                hash: parent_id,
                index: 0,
            }],
        );
        transaction_dependencies.add(
            grandchild_id,
            vec![OutPoint {
                hash: child_id,
                index: 0,
            }],
        );

        let mempool_object = MempoolObject::from_verified_unmined_tx(
            &transactions[0],
            &transactions,
            &transaction_dependencies,
        );

        assert_eq!(
            mempool_object.descendantcount, 2,
            "verbose mempool output counts only the direct child plus itself today"
        );
        assert_eq!(
            mempool_object.descendantsize,
            (transactions[0].transaction.size + transactions[1].transaction.size) as u64,
            "verbose mempool output includes only direct child size today"
        );
        assert_eq!(
            mempool_object.descendantfees, 2_000_000,
            "verbose mempool output includes only direct child fee today"
        );
    }

    #[test]
    fn verbose_mempool_object_rebuilds_lookup_for_each_transaction_today() {
        let transactions = Network::Mainnet
            .unmined_transactions_in_blocks(..)
            .filter(|tx| !tx.transaction.transaction.is_coinbase())
            .take(8)
            .collect::<Vec<_>>();

        assert_eq!(
            transactions.len(),
            8,
            "test vectors should provide at least eight non-coinbase mempool transactions"
        );

        let transaction_dependencies = TransactionDependencies::default();
        reset_transaction_lookup_map_counters();

        for transaction in &transactions {
            let _ = MempoolObject::from_verified_unmined_tx(
                transaction,
                &transactions,
                &transaction_dependencies,
            );
        }

        let (lookup_builds, lookup_inputs) = transaction_lookup_map_counters();
        assert_eq!(
            lookup_builds,
            transactions.len(),
            "verbose object assembly rebuilds the full lookup map once per transaction today"
        );
        assert_eq!(
            lookup_inputs,
            transactions.len() * transactions.len(),
            "each lookup-map rebuild iterates the full transaction slice"
        );
    }
}
