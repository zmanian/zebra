//! Genesis block construction and deserialization.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use hex::FromHex;

use crate::{
    amount::{Amount, NonNegative},
    block::{header::ZCASH_BLOCK_VERSION, merkle, Block, Header, Height},
    fmt::HexDebug,
    parameters::{Network, GENESIS_PREVIOUS_BLOCK_HASH},
    serialization::ZcashDeserializeInto,
    transaction::{LockTime, Transaction},
    transparent::{self, CoinbaseData, Input, Output, Script},
    work::{
        difficulty::{ExpandedDifficulty, U256},
        equihash::Solution,
    },
};

use transparent::GENESIS_COINBASE_DATA;

/// The Bitcoin genesis pubkey script (uncompressed pubkey + OP_CHECKSIG).
///
/// This is the output script used in the Zcash genesis coinbase transaction,
/// inherited from Bitcoin's genesis block.
const GENESIS_OUTPUT_SCRIPT: [u8; 67] = [
    0x41, // OP_DATA_65 (push 65 bytes)
    0x04, 0x67, 0x8a, 0xfd, 0xb0, 0xfe, 0x55, 0x48, 0x27, 0x19, 0x67, 0xf1, 0xa6, 0x71, 0x30, 0xb7,
    0x10, 0x5c, 0xd6, 0xa8, 0x28, 0xe0, 0x39, 0x09, 0xa6, 0x79, 0x62, 0xe0, 0xea, 0x1f, 0x61, 0xde,
    0xb6, 0x49, 0xf6, 0xbc, 0x3f, 0x4c, 0xef, 0x38, 0xc4, 0xf3, 0x55, 0x04, 0xe5, 0x1e, 0xc1, 0x12,
    0xde, 0x5c, 0x38, 0x4d, 0xf7, 0xba, 0x0b, 0x8d, 0x57, 0x8a, 0x4c, 0x70, 0x2b, 0x6b, 0xf1, 0x1d,
    0x5f, // end of pubkey
    0xac, // OP_CHECKSIG
];

/// Genesis block for Regtest, copied from zcashd via `getblock 0 0` RPC method
pub fn regtest_genesis_block() -> Arc<Block> {
    let regtest_genesis_block_bytes =
        <Vec<u8>>::from_hex(include_str!("genesis/block-regtest-0-000-000.txt").trim())
            .expect("Block bytes are in valid hex representation");

    regtest_genesis_block_bytes
        .zcash_deserialize_into()
        .map(Arc::new)
        .expect("hard-coded Regtest genesis block data must deserialize successfully")
}

/// Returns the genesis block for the given network, if one needs to be
/// committed at startup.
///
/// Returns `Some` for:
/// - Regtest (hardcoded genesis block)
/// - Custom testnets with a generated genesis block
///
/// Returns `None` for:
/// - Mainnet (receives genesis through checkpoint sync)
/// - Default Testnet (receives genesis through checkpoint sync)
pub fn genesis_block_for_network(network: &Network) -> Option<Arc<Block>> {
    match network {
        Network::Mainnet => None,
        Network::Testnet(params) => {
            // First check if this network has a stored genesis block
            if let Some(block) = params.genesis_block() {
                return Some(block.clone());
            }
            // Regtest uses the hardcoded genesis block
            if params.is_regtest() {
                return Some(regtest_genesis_block());
            }
            // Default testnet uses checkpoint sync
            None
        }
    }
}

/// Construct a genesis block programmatically, matching zcashd's `CreateGenesisBlock()`.
///
/// # Arguments
///
/// * `timestamp` - Unix epoch timestamp (nTime) for the block header
/// * `difficulty_target_bytes` - The 256-bit difficulty target in big-endian byte order,
///   which is converted to the compact nBits representation for the header
/// * `nonce_and_solution` - Optional Equihash nonce (32 bytes) and solution bytes.
///   If `None`, the nonce and solution are set to all zeros (suitable for regtest
///   where proof-of-work is not validated).
///
/// The genesis block contains:
/// - A single V1 coinbase transaction with `Height(0)`, `GENESIS_COINBASE_DATA`,
///   and a zero-value transparent output paying to the Bitcoin genesis pubkey script
/// - Block version 4, null previous block hash, the computed merkle root,
///   null commitment bytes, and the provided timestamp/difficulty/nonce/solution
///
/// # Panics
///
/// If the difficulty target converts to an invalid `CompactDifficulty` (zero or negative).
pub fn create_genesis_block(
    timestamp: i64,
    difficulty_target_bytes: [u8; 32],
    nonce_and_solution: Option<([u8; 32], Vec<u8>)>,
) -> Arc<Block> {
    // Build the coinbase input.
    // The genesis block does not use BIP34 height encoding; the full scriptSig
    // is GENESIS_COINBASE_DATA. Zebra's serialization handles this: at Height(0)
    // with this exact data, write_coinbase_height writes nothing, then the data
    // is appended as-is.
    let coinbase_input = Input::Coinbase {
        height: Height(0),
        data: CoinbaseData::for_genesis(GENESIS_COINBASE_DATA.to_vec()),
        sequence: 0xffff_ffff,
    };

    // Build the coinbase output: zero value to the Bitcoin genesis pubkey script
    let coinbase_output = Output::new(
        Amount::<NonNegative>::zero(),
        Script::new(&GENESIS_OUTPUT_SCRIPT),
    );

    // Build the V1 coinbase transaction
    let coinbase_tx = Transaction::V1 {
        inputs: vec![coinbase_input],
        outputs: vec![coinbase_output],
        lock_time: LockTime::Height(Height(0)),
    };

    let coinbase_tx = Arc::new(coinbase_tx);

    // Compute the merkle root from the single transaction
    let merkle_root: merkle::Root = [coinbase_tx.clone()].iter().collect();

    // Convert big-endian difficulty target to CompactDifficulty
    let expanded = ExpandedDifficulty::from(U256::from_big_endian(&difficulty_target_bytes));
    let difficulty_threshold = expanded.to_compact();

    // Extract nonce and solution, defaulting to zeros
    let (nonce, solution) = match nonce_and_solution {
        Some((n, s)) => {
            let sol = Solution::from_bytes(&s)
                .expect("provided solution bytes must be a valid Equihash solution size");
            (n, sol)
        }
        None => {
            // Default: all-zero nonce and regtest-sized zero solution
            ([0u8; 32], Solution::Regtest([0u8; 36]))
        }
    };

    let header = Header {
        version: ZCASH_BLOCK_VERSION,
        previous_block_hash: GENESIS_PREVIOUS_BLOCK_HASH,
        merkle_root,
        commitment_bytes: HexDebug([0u8; 32]),
        time: Utc
            .timestamp_opt(timestamp, 0)
            .single()
            .expect("timestamp must be a valid Unix epoch time"),
        difficulty_threshold,
        nonce: HexDebug(nonce),
        solution,
    };

    Arc::new(Block {
        header: Arc::new(header),
        transactions: vec![coinbase_tx],
    })
}

/// Construct and mine a genesis block by finding a valid Equihash solution.
///
/// This calls [`create_genesis_block()`] to build the block template, then uses
/// [`Solution::solve()`] to find a valid nonce and Equihash solution that meets
/// the given difficulty target.
///
/// # Arguments
///
/// * `timestamp` - Unix epoch timestamp (nTime) for the block header
/// * `difficulty_target_bytes` - The 256-bit difficulty target in big-endian byte order
///
/// # Panics
///
/// If the solver is cancelled (which cannot happen here since we use a no-op cancel function).
///
/// # Performance
///
/// This function is CPU and memory-intensive. It uses 144 MB of RAM and one CPU core.
/// With an easy difficulty target it completes quickly; with a hard target it may run
/// for minutes or hours.
#[cfg(feature = "internal-miner")]
pub fn create_and_mine_genesis_block(
    timestamp: i64,
    difficulty_target_bytes: [u8; 32],
) -> Arc<Block> {
    use crate::work::equihash::{SolverCancelled, SOLUTION_SIZE};

    // Create a genesis block template with null nonce/solution
    let template = create_genesis_block(timestamp, difficulty_target_bytes, None);

    // Build a header with a Common solution (1344 bytes) for the solver template.
    // The solver is hardcoded to (200, 9) parameters which produce Common solutions.
    let mut header = (*template.header).clone();
    header.solution = Solution::Common([0u8; SOLUTION_SIZE]);

    // Solve for a valid nonce and equihash solution. The cancel function never cancels.
    let solved_headers = Solution::solve(header, || Ok::<(), SolverCancelled>(()))
        .expect("solver should not be cancelled with a no-op cancel function");

    // Take the first valid solved header
    let solved_header = solved_headers
        .into_iter()
        .next()
        .expect("AtLeastOne guarantees at least one solved header");

    Arc::new(Block {
        header: Arc::new(solved_header),
        transactions: template.transactions.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{serialization::ZcashSerialize, work::difficulty::CompactDifficulty};

    /// The regtest genesis block parameters from zcashd:
    /// - Timestamp: 1296688602 (2011-02-02 23:16:42 UTC)
    /// - Difficulty target: 0x0f0f0f0f... (regtest easiest difficulty)
    /// - Nonce: 0x0000...0009 (little-endian 9)
    /// - Solution: the regtest genesis Equihash solution
    const REGTEST_TIMESTAMP: i64 = 1296688602;

    /// Regtest difficulty target in big-endian (corresponds to nBits 0x200f0f0f).
    fn regtest_difficulty_target() -> [u8; 32] {
        let mut target = [0u8; 32];
        // Derived from the known compact nBits value 0x200f0f0f.
        let compact = CompactDifficulty(0x200f_0f0f);
        let expanded = compact.to_expanded().expect("regtest difficulty is valid");
        let expanded_u256: U256 = expanded.into();
        let bytes = expanded_u256.to_big_endian();
        target.copy_from_slice(&bytes);
        target
    }

    /// Extract the regtest genesis nonce and solution from the hardcoded block.
    fn regtest_nonce_and_solution() -> ([u8; 32], Vec<u8>) {
        let block = regtest_genesis_block();
        let nonce = *block.header.nonce;
        let solution_bytes = match block.header.solution {
            Solution::Regtest(bytes) => bytes.to_vec(),
            Solution::Common(bytes) => bytes.to_vec(),
        };
        (nonce, solution_bytes)
    }

    #[test]
    fn created_genesis_coinbase_matches_hardcoded() {
        let hardcoded = regtest_genesis_block();
        let (nonce, solution) = regtest_nonce_and_solution();

        let created = create_genesis_block(
            REGTEST_TIMESTAMP,
            regtest_difficulty_target(),
            Some((nonce, solution)),
        );

        // Compare coinbase transaction serialization
        let hardcoded_coinbase = &hardcoded.transactions[0];
        let created_coinbase = &created.transactions[0];

        let mut hardcoded_bytes = Vec::new();
        hardcoded_coinbase
            .zcash_serialize(&mut hardcoded_bytes)
            .expect("serialization succeeds");

        let mut created_bytes = Vec::new();
        created_coinbase
            .zcash_serialize(&mut created_bytes)
            .expect("serialization succeeds");

        assert_eq!(
            hardcoded_bytes, created_bytes,
            "coinbase transaction bytes must match"
        );

        // Also compare transaction hashes
        assert_eq!(
            hardcoded_coinbase.hash(),
            created_coinbase.hash(),
            "coinbase transaction hashes must match"
        );
    }

    #[test]
    fn created_genesis_merkle_root_matches_hardcoded() {
        let hardcoded = regtest_genesis_block();
        let (nonce, solution) = regtest_nonce_and_solution();

        let created = create_genesis_block(
            REGTEST_TIMESTAMP,
            regtest_difficulty_target(),
            Some((nonce, solution)),
        );

        assert_eq!(
            hardcoded.header.merkle_root, created.header.merkle_root,
            "merkle roots must match"
        );
    }

    #[test]
    fn created_genesis_header_fields_match_hardcoded() {
        let hardcoded = regtest_genesis_block();
        let (nonce, solution) = regtest_nonce_and_solution();

        let created = create_genesis_block(
            REGTEST_TIMESTAMP,
            regtest_difficulty_target(),
            Some((nonce, solution)),
        );

        assert_eq!(
            hardcoded.header.version, created.header.version,
            "block versions must match"
        );
        assert_eq!(
            hardcoded.header.previous_block_hash, created.header.previous_block_hash,
            "previous block hashes must match"
        );
        assert_eq!(
            hardcoded.header.time, created.header.time,
            "timestamps must match"
        );
        assert_eq!(
            hardcoded.header.difficulty_threshold, created.header.difficulty_threshold,
            "difficulty thresholds must match"
        );
        assert_eq!(
            hardcoded.header.nonce, created.header.nonce,
            "nonces must match"
        );
        assert_eq!(
            hardcoded.header.solution, created.header.solution,
            "solutions must match"
        );
        assert_eq!(
            hardcoded.header.commitment_bytes, created.header.commitment_bytes,
            "commitment bytes must match"
        );
    }

    #[test]
    fn created_genesis_block_hash_matches_hardcoded() {
        let hardcoded = regtest_genesis_block();
        let (nonce, solution) = regtest_nonce_and_solution();

        let created = create_genesis_block(
            REGTEST_TIMESTAMP,
            regtest_difficulty_target(),
            Some((nonce, solution)),
        );

        assert_eq!(hardcoded.hash(), created.hash(), "block hashes must match");
    }

    #[test]
    fn created_genesis_serialization_roundtrips() {
        let hardcoded = regtest_genesis_block();
        let (nonce, solution) = regtest_nonce_and_solution();

        let created = create_genesis_block(
            REGTEST_TIMESTAMP,
            regtest_difficulty_target(),
            Some((nonce, solution)),
        );

        // Serialize both and compare
        let mut hardcoded_bytes = Vec::new();
        hardcoded
            .zcash_serialize(&mut hardcoded_bytes)
            .expect("hardcoded block serializes");

        let mut created_bytes = Vec::new();
        created
            .zcash_serialize(&mut created_bytes)
            .expect("created block serializes");

        assert_eq!(
            hardcoded_bytes, created_bytes,
            "full block serialization must match"
        );
    }

    /// Test that `create_and_mine_genesis_block` produces a block with a valid
    /// Equihash solution that passes verification.
    #[test]
    #[cfg(feature = "internal-miner")]
    fn mined_genesis_block_has_valid_solution() {
        let _init_guard = zebra_test::init();

        // Use an extremely easy difficulty target so the solver finds a solution quickly.
        // 0x0f repeated 32 times means nearly all hashes will pass.
        let easy_target = [0x0fu8; 32];
        let timestamp = 1296688602i64; // same as regtest

        let block = super::create_and_mine_genesis_block(timestamp, easy_target);

        // Verify the equihash solution is valid
        block
            .header
            .solution
            .check(&block.header)
            .expect("mined genesis block must have a valid equihash solution");

        // Verify the block meets the difficulty target
        let hash = block.hash();
        let expanded = block
            .header
            .difficulty_threshold
            .to_expanded()
            .expect("difficulty threshold is valid");
        assert!(
            hash <= expanded,
            "mined block hash must meet the difficulty target"
        );

        // Verify the solution is a Common (1344-byte) solution
        assert!(
            matches!(
                block.header.solution,
                crate::work::equihash::Solution::Common(_)
            ),
            "mined genesis block must have a Common equihash solution"
        );
    }
}
