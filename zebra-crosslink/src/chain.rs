//! Core Zcash Crosslink data structures
//!
//! This crate deals only with in-memory state validation and excludes I/O, tokio, services, etc...
//!
//! This crate is named similarly to [zebra_chain] since it has a similar scope. In a mature crosslink-enabled Zebra these two crates may be merged.
#![deny(unsafe_code, missing_docs)]

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    io::{Read, Write},
};
use thiserror::Error;
use tracing::error;
use zebra_chain::block::Header as BcBlockHeader;

use zebra_chain::serialization::{
    ReadZcashExt, SerializationError, ZcashDeserialize, ZcashSerialize,
};

use crate::dynamic_sigma::{
    validate_dynamic_sigma_evidence, DynamicSigmaEvidenceError, DynamicSigmaParameters,
    DynamicSigmaProposalEvidence,
};
use crate::FatPointerToBftBlock2;

/// The BFT block content for Crosslink
///
/// # Constructing [BftBlock]s
///
/// A [BftBlock] may be constructed from a node's local view in order to create a new BFT proposal, or they may be constructed from unknown sources across a network protocol.
///
/// To construct a [BftBlock] for a new BFT proposal, build a [Vec] of [BcBlockHeader] values, starting from the latest known PoW tip and traversing back in time (following [previous_block_hash](BcBlockHeader::previous_block_hash)) until exactly [bc_confirmation_depth_sigma](ZcashCrosslinkParameters::bc_confirmation_depth_sigma) headers are collected, then pass this to [BftBlock::try_from].
///
/// To construct from an untrusted source, call the same [BftBlock::try_from].
///
/// ## Validation and Limitations
///
/// The [BftBlock::try_from] method is the only way to construct [BftBlock] values and performs the following validation internally:
///
/// 1. The number of headers matches the expected protocol confirmation depth, [bc_confirmation_depth_sigma](ZcashCrosslinkParameters::bc_confirmation_depth_sigma).
/// 2. The [version](BcBlockHeader::version) field is a known expected value.
/// 3. The headers are in the correct order given the [previous_block_hash](BcBlockHeader::previous_block_hash) fields.
/// 4. The PoW solutions validate.
///
/// These validations use *immediate data* and are *stateless*, and in particular the following stateful validations are **NOT** performed:
///
/// 1. The [difficulty_threshold](BcBlockHeader::difficulty_threshold) is within correct bounds for the Difficulty Adjustment Algorithm.
/// 2. The [time](BcBlockHeader::time) field is within correct bounds.
/// 3. The [merkle_root](BcBlockHeader::merkle_root) field is sensible.
///
/// No other validations are performed.
///
/// **TODO:** Ensure deserialization delegates to [BftBlock::try_from].
///
/// ## Design Notes
///
/// This *assumes* is is more natural to fetch the latest BC tip in Zebra, then to iterate to parent blocks, appending each to the [Vec]. This means the in-memory header order is *reversed from the specification* [^1]:
///
/// > Each bft‑proposal has, in addition to origbft‑proposal fields, a headers_bc field containing a sequence of exactly σ bc‑headers (zero‑indexed, deepest first).
///
/// The [TryFrom] impl performs internal validations and is the only way to construct a [BftBlock], whether locally generated or from an unknown source. This is the safest design, though potentially less efficient.
///
/// # References
///
/// [^1]: [Zcash Trailing Finality Layer §3.3.3 Structural Additions](https://electric-coin-company.github.io/tfl-book/design/crosslink/construction.html#structural-additions)
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)] //, Serialize, Deserialize)]
pub struct BftBlock {
    /// The Version Number
    pub version: u32,
    /// The Height of this BFT Payload
    // @Zooko: possibly not unique, may be bug-prone, maybe remove...
    pub height: u32,
    /// Hash of the previous BFT Block.
    pub previous_block_fat_ptr: FatPointerToBftBlock2,
    /// The height of the PoW block that is the finalization candidate.
    pub finalization_candidate_height: u32,
    /// The PoW Headers
    // @Zooko: PoPoW?
    pub headers: Vec<BcBlockHeader>,
}

/// Magic prefix for serialized dynamic-sigma BFT block payloads.
pub const DYNAMIC_SIGMA_BFT_BLOCK_PAYLOAD_MAGIC: [u8; 8] = *b"CLDSIG01";

/// A dynamic-sigma BFT payload carrying proposal evidence and the BFT block.
///
/// This is a transport envelope for the dynamic-sigma variant. It is distinct
/// from the fixed-sigma [BftBlock] serialization so a dynamic-sigma payload
/// cannot be accidentally parsed as a fixed-sigma block value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicSigmaBftBlockPayload {
    /// Proposal-carried evidence used to validate the selected sigma.
    pub evidence: DynamicSigmaProposalEvidence,
    /// The BFT block constructed using the evidence-selected sigma.
    pub block: BftBlock,
}

/// A decoded Tenderlink BFT block payload.
///
/// This routes the legacy fixed-sigma block serialization and the tagged
/// dynamic-sigma envelope without letting one format be accidentally parsed as
/// the other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BftBlockPayload {
    /// Legacy fixed-sigma BFT block bytes.
    FixedSigma(BftBlock),
    /// Tagged dynamic-sigma evidence plus BFT block bytes.
    DynamicSigma(DynamicSigmaBftBlockPayload),
}

impl BftBlockPayload {
    /// Decode a Tenderlink payload from bytes.
    pub fn zcash_deserialize_from_slice(bytes: &[u8]) -> Result<Self, SerializationError> {
        if bytes.starts_with(&DYNAMIC_SIGMA_BFT_BLOCK_PAYLOAD_MAGIC) {
            DynamicSigmaBftBlockPayload::zcash_deserialize(bytes).map(Self::DynamicSigma)
        } else {
            BftBlock::zcash_deserialize(bytes).map(Self::FixedSigma)
        }
    }

    /// Return the carried BFT block regardless of payload variant.
    pub fn block(&self) -> &BftBlock {
        match self {
            Self::FixedSigma(block) => block,
            Self::DynamicSigma(payload) => &payload.block,
        }
    }
}

impl DynamicSigmaBftBlockPayload {
    /// Attempt to construct a dynamic-sigma payload from proposal-carried
    /// evidence and BFT block fields.
    pub fn try_from_with_evidence(
        dynamic_sigma_params: DynamicSigmaParameters,
        dynamic_sigma_evidence: DynamicSigmaProposalEvidence,
        height: u32,
        previous_block_fat_ptr: FatPointerToBftBlock2,
        finalization_candidate_height: u32,
        headers: Vec<BcBlockHeader>,
    ) -> Result<Self, InvalidDynamicSigmaBftBlock> {
        let block = BftBlock::try_from_with_dynamic_sigma_evidence(
            dynamic_sigma_params,
            dynamic_sigma_evidence,
            height,
            previous_block_fat_ptr,
            finalization_candidate_height,
            headers,
        )?;

        Ok(Self {
            evidence: dynamic_sigma_evidence,
            block,
        })
    }

    /// Validate that this payload's evidence permits exactly the carried block.
    pub fn validate(
        &self,
        dynamic_sigma_params: DynamicSigmaParameters,
    ) -> Result<(), InvalidDynamicSigmaBftBlock> {
        let evidence_validated_block = BftBlock::try_from_with_dynamic_sigma_evidence(
            dynamic_sigma_params,
            self.evidence,
            self.block.height,
            self.block.previous_block_fat_ptr.clone(),
            self.block.finalization_candidate_height,
            self.block.headers.clone(),
        )?;

        let expected_bytes = evidence_validated_block
            .zcash_serialize_to_vec()
            .map_err(|_| InvalidDynamicSigmaBftBlock::PayloadBlockMismatch)?;
        let actual_bytes = self
            .block
            .zcash_serialize_to_vec()
            .map_err(|_| InvalidDynamicSigmaBftBlock::PayloadBlockMismatch)?;

        if expected_bytes != actual_bytes {
            return Err(InvalidDynamicSigmaBftBlock::PayloadBlockMismatch);
        }

        Ok(())
    }
}

impl ZcashSerialize for DynamicSigmaBftBlockPayload {
    fn zcash_serialize<W: Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        writer.write_all(&DYNAMIC_SIGMA_BFT_BLOCK_PAYLOAD_MAGIC)?;
        self.evidence.zcash_serialize(&mut writer)?;
        self.block.zcash_serialize(&mut writer)?;

        Ok(())
    }
}

impl ZcashDeserialize for DynamicSigmaBftBlockPayload {
    fn zcash_deserialize<R: Read>(mut reader: R) -> Result<Self, SerializationError> {
        let mut magic = [0u8; DYNAMIC_SIGMA_BFT_BLOCK_PAYLOAD_MAGIC.len()];
        reader.read_exact(&mut magic)?;
        if magic != DYNAMIC_SIGMA_BFT_BLOCK_PAYLOAD_MAGIC {
            return Err(SerializationError::Parse(
                "invalid dynamic sigma BFT payload magic",
            ));
        }

        Ok(Self {
            evidence: DynamicSigmaProposalEvidence::zcash_deserialize(&mut reader)?,
            block: BftBlock::zcash_deserialize(&mut reader)?,
        })
    }
}

impl ZcashSerialize for BftBlock {
    #[allow(clippy::unwrap_in_result)]
    fn zcash_serialize<W: std::io::Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        writer.write_u32::<LittleEndian>(self.version)?;
        writer.write_u32::<LittleEndian>(self.height)?;
        self.previous_block_fat_ptr.zcash_serialize(&mut writer);
        writer.write_u32::<LittleEndian>(self.finalization_candidate_height)?;
        writer.write_u32::<LittleEndian>(self.headers.len().try_into().unwrap())?;
        for header in &self.headers {
            header.zcash_serialize(&mut writer)?;
        }
        Ok(())
    }
}

impl ZcashDeserialize for BftBlock {
    fn zcash_deserialize<R: std::io::Read>(mut reader: R) -> Result<Self, SerializationError> {
        let version = reader.read_u32::<LittleEndian>()?;
        let height = reader.read_u32::<LittleEndian>()?;
        let previous_block_fat_ptr = FatPointerToBftBlock2::zcash_deserialize(&mut reader)?;
        let finalization_candidate_height = reader.read_u32::<LittleEndian>()?;
        let header_count = reader.read_u32::<LittleEndian>()?;
        if header_count > 2048 {
            // Fail on unreasonably large number.
            return Err(SerializationError::Parse(
                "header_count was greater than 2048.",
            ));
        }
        let mut array = Vec::new();
        for i in 0..header_count {
            array.push(zebra_chain::block::Header::zcash_deserialize(&mut reader)?);
        }

        Ok(BftBlock {
            version,
            height,
            previous_block_fat_ptr,
            finalization_candidate_height,
            headers: array,
        })
    }
}

impl BftBlock {
    /// Refer to the [BcBlockHeader] that is the finalization candidate for this block
    ///
    /// **UNVERIFIED:** The finalization_candidate of a final [BftBlock] is finalized.
    pub fn finalization_candidate(&self) -> &BcBlockHeader {
        &self.headers.last().expect("Vec should never be empty")
    }

    /// Attempt to construct a [BftBlock] from headers while performing immediate validations; see [BftBlock] type docs
    pub fn try_from(
        params: &ZcashCrosslinkParameters,
        height: u32,
        previous_block_fat_ptr: FatPointerToBftBlock2,
        finalization_candidate_height: u32,
        headers: Vec<BcBlockHeader>,
    ) -> Result<Self, InvalidBftBlock> {
        Self::try_from_with_confirmation_depth(
            params.bc_confirmation_depth_sigma,
            height,
            previous_block_fat_ptr,
            finalization_candidate_height,
            headers,
        )
    }

    /// Attempt to construct a [BftBlock] using a selected confirmation depth.
    ///
    /// This is the dynamic-sigma construction hook: the existing
    /// [BftBlock::try_from] path still reads the fixed protocol sigma from
    /// [ZcashCrosslinkParameters], while future proposal-carried controller
    /// evidence can validate the selected sigma first through
    /// [BftBlock::try_from_with_dynamic_sigma_evidence] and then call this
    /// method.
    pub fn try_from_with_confirmation_depth(
        expected_confirmation_depth: u64,
        height: u32,
        previous_block_fat_ptr: FatPointerToBftBlock2,
        finalization_candidate_height: u32,
        headers: Vec<BcBlockHeader>,
    ) -> Result<Self, InvalidBftBlock> {
        let expected = expected_confirmation_depth;
        let actual = headers.len() as u64;
        if actual != expected {
            return Err(InvalidBftBlock::IncorrectConfirmationDepth { expected, actual });
        }

        error!("not yet implemented: all the documented validations");

        Ok(BftBlock {
            version: 1,
            height,
            previous_block_fat_ptr,
            finalization_candidate_height,
            headers,
        })
    }

    /// Attempt to construct a [BftBlock] from proposal-carried dynamic-sigma
    /// evidence.
    ///
    /// The evidence is validated against the shared controller parameters first.
    /// If the proposer selected a valid ladder sigma that is equal to or more
    /// conservative than the controller floor, the selected sigma becomes the
    /// required header depth for this proposal.
    pub fn try_from_with_dynamic_sigma_evidence(
        dynamic_sigma_params: DynamicSigmaParameters,
        dynamic_sigma_evidence: DynamicSigmaProposalEvidence,
        height: u32,
        previous_block_fat_ptr: FatPointerToBftBlock2,
        finalization_candidate_height: u32,
        headers: Vec<BcBlockHeader>,
    ) -> Result<Self, InvalidDynamicSigmaBftBlock> {
        validate_dynamic_sigma_evidence(dynamic_sigma_params, dynamic_sigma_evidence)
            .map_err(InvalidDynamicSigmaBftBlock::DynamicSigmaEvidence)?;

        Self::try_from_with_confirmation_depth(
            dynamic_sigma_evidence.selected_sigma,
            height,
            previous_block_fat_ptr,
            finalization_candidate_height,
            headers,
        )
        .map_err(InvalidDynamicSigmaBftBlock::BftBlock)
    }

    /// Hash for the block
    /// ([BftBlock::hash]).
    pub fn blake3_hash(&self) -> Blake3Hash {
        self.into()
    }

    /// Just the hash of the previous block, which identifies it but does not provide any
    /// guarantees. Consider using the [`previous_block_fat_ptr`] instead
    pub fn previous_block_hash(&self) -> Blake3Hash {
        self.previous_block_fat_ptr.points_at_block_hash()
    }
}

impl<'a> From<&'a BftBlock> for Blake3Hash {
    fn from(block: &'a BftBlock) -> Self {
        let mut hash_writer = if *crate::TEST_MODE.lock().unwrap() {
            // Note(Sam): Only until we regenerate the test data.
            blake3::Hasher::new()
        } else {
            blake3::Hasher::new_keyed(&tenderlink::HashKeys::default().value_id.0)
        };
        block
            .zcash_serialize(&mut hash_writer)
            .expect("Sha256dWriter is infallible");
        Self(hash_writer.finalize().into())
    }
}

/// Validation error for [BftBlock]
#[derive(Debug, Error)]
pub enum InvalidBftBlock {
    /// An incorrect number of headers was present
    #[error(
        "invalid confirmation depth: Crosslink requires {expected} while {actual} were present"
    )]
    IncorrectConfirmationDepth {
        /// The expected number of headers, as per [bc_confirmation_depth_sigma](ZcashCrosslinkParameters::bc_confirmation_depth_sigma)
        expected: u64,
        /// The number of headers present
        actual: u64,
    },
}

/// Validation error for a dynamic-sigma [BftBlock] proposal.
#[derive(Debug, Error)]
pub enum InvalidDynamicSigmaBftBlock {
    /// The proposal-carried dynamic-sigma evidence is invalid.
    #[error("invalid dynamic sigma evidence: {0:?}")]
    DynamicSigmaEvidence(DynamicSigmaEvidenceError),
    /// The BFT block content does not satisfy the selected sigma.
    #[error("invalid dynamic sigma BFT block: {0}")]
    BftBlock(InvalidBftBlock),
    /// The payload block does not match the block permitted by the evidence.
    #[error("dynamic sigma payload block does not match the evidence-validated block")]
    PayloadBlockMismatch,
}

/// Zcash Crosslink protocol parameters
///
/// This is provided as a trait so that downstream users can define or plug in their own alternative parameters.
///
/// Ref: [Zcash Trailing Finality Layer §3.3.3 Parameters](https://electric-coin-company.github.io/tfl-book/design/crosslink/construction.html#parameters)
#[derive(Clone, Debug)]
pub struct ZcashCrosslinkParameters {
    /// The best-chain confirmation depth, `σ`
    ///
    /// At least this many PoW blocks must be atop the PoW block used to obtain a finalized view.
    pub bc_confirmation_depth_sigma: u64,

    /// The depth of unfinalized PoW blocks past which "Stalled Mode" activates, `L`
    ///
    /// Quoting from [Zcash Trailing Finality Layer §3.3.3 Stalled Mode](https://electric-coin-company.github.io/tfl-book/design/crosslink/construction.html#stalled-mode):
    ///
    /// > In practice, L should be at least 2σ.
    pub finalization_gap_bound: u64,
}

/// Crosslink parameters chosed for prototyping / testing
///
/// <div class="warning">No verification has been done on the security or performance of these parameters.</div>
pub const PROTOTYPE_PARAMETERS: ZcashCrosslinkParameters = ZcashCrosslinkParameters {
    bc_confirmation_depth_sigma: 3,
    finalization_gap_bound: 7,
};

/// A BLAKE3 hash.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Serialize, Deserialize)]
pub struct Blake3Hash(pub [u8; 32]);

impl std::fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for &b in self.0.iter() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for Blake3Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for &b in self.0.iter() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl ZcashSerialize for Blake3Hash {
    #[allow(clippy::unwrap_in_result)]
    fn zcash_serialize<W: std::io::Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        writer.write_all(&self.0);
        Ok(())
    }
}

impl ZcashDeserialize for Blake3Hash {
    fn zcash_deserialize<R: std::io::Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(Blake3Hash(reader.read_32_bytes()?))
    }
}

/// A BFT block and the fat pointer that shows it has been signed
#[derive(Clone, Debug)]
pub struct BftBlockAndFatPointerToIt {
    /// A BFT block
    pub block: BftBlock,
    /// The fat pointer to block, showing it has been signed
    pub fat_ptr: FatPointerToBftBlock2,
}

impl ZcashDeserialize for BftBlockAndFatPointerToIt {
    fn zcash_deserialize<R: std::io::Read>(mut reader: R) -> Result<Self, SerializationError> {
        Ok(BftBlockAndFatPointerToIt {
            block: BftBlock::zcash_deserialize(&mut reader)?,
            fat_ptr: FatPointerToBftBlock2::zcash_deserialize(&mut reader)?,
        })
    }
}

impl ZcashSerialize for BftBlockAndFatPointerToIt {
    fn zcash_serialize<W: std::io::Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        self.block.zcash_serialize(&mut writer);
        self.fat_ptr.zcash_serialize(&mut writer);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_sigma::{
        DynamicSigmaEvidenceError, DynamicSigmaParameters, DynamicSigmaProposalEvidence,
        DynamicSigmaRawTelemetry, RollbackRiskCurve, TelemetryEstimateMargins,
    };
    use chrono::Utc;
    use zebra_chain::{
        block::{merkle::Root, FatPointerToBftBlock, Hash as BlockHash, Header},
        fmt::HexDebug,
        work::{difficulty::INVALID_COMPACT_DIFFICULTY, equihash::Solution},
    };

    fn test_header(previous_block_hash: BlockHash) -> Header {
        Header {
            version: 4,
            previous_block_hash,
            merkle_root: Root([0; 32]),
            commitment_bytes: HexDebug([0; 32]),
            time: Utc::now(),
            difficulty_threshold: INVALID_COMPACT_DIFFICULTY,
            nonce: HexDebug([0; 32]),
            solution: Solution::for_proposal(),
            fat_pointer_to_bft_block: FatPointerToBftBlock::null(),
        }
    }

    fn dynamic_sigma_params() -> DynamicSigmaParameters {
        DynamicSigmaParameters {
            base_sigma: 1,
            raised_sigma: 3,
            max_sigma: 6,
            target_hash_participation_pct: 67,
            critical_hash_participation_pct: 50,
            max_acceptable_rollback_risk_ppm: 25,
            coverage_risk_weight: 2,
            round_failure_risk_weight: 3,
            block_interval_variance_risk_weight: 1,
            reorg_depth_risk_weight: 5,
            risk_score_raised_threshold: 60,
            risk_score_max_threshold: 100,
        }
    }

    fn dynamic_sigma_evidence(
        participating_hash_work: u128,
        selected_sigma: u64,
    ) -> DynamicSigmaProposalEvidence {
        DynamicSigmaProposalEvidence {
            raw_telemetry: DynamicSigmaRawTelemetry {
                total_hash_work: 100,
                crosslink_participating_hash_work: participating_hash_work,
                total_tenderlink_rounds: 10,
                failed_tenderlink_rounds: 0,
                measured_block_interval_variance_pct: 0,
                measured_observed_reorg_depth: 0,
                rollback_risk: RollbackRiskCurve {
                    base_sigma_ppm: 80,
                    raised_sigma_ppm: 20,
                    max_sigma_ppm: 2,
                },
                value_at_risk_units: 1_000,
                max_acceptable_expected_loss_units: 1,
            },
            margins: TelemetryEstimateMargins {
                coverage_risk_margin_pct: 2,
                round_failure_margin_pct: 0,
            },
            selected_sigma,
        }
    }

    #[test]
    fn try_from_with_confirmation_depth_uses_selected_sigma() {
        let err = BftBlock::try_from_with_confirmation_depth(
            6,
            1,
            FatPointerToBftBlock2::null(),
            10,
            Vec::new(),
        )
        .expect_err("empty headers should not satisfy selected sigma 6");

        assert!(matches!(
            err,
            InvalidBftBlock::IncorrectConfirmationDepth {
                expected: 6,
                actual: 0,
            }
        ));
    }

    #[test]
    fn try_from_keeps_using_fixed_parameter_sigma() {
        let params = ZcashCrosslinkParameters {
            bc_confirmation_depth_sigma: 3,
            finalization_gap_bound: 7,
        };
        let err = BftBlock::try_from(&params, 1, FatPointerToBftBlock2::null(), 10, Vec::new())
            .expect_err("empty headers should not satisfy fixed sigma 3");

        assert!(matches!(
            err,
            InvalidBftBlock::IncorrectConfirmationDepth {
                expected: 3,
                actual: 0,
            }
        ));
    }

    #[test]
    fn try_from_with_confirmation_depth_accepts_matching_header_count() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
        ];

        let block = BftBlock::try_from_with_confirmation_depth(
            2,
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers.clone(),
        )
        .expect("matching selected sigma should be accepted");

        assert_eq!(block.headers, headers);
    }

    #[test]
    fn try_from_with_dynamic_sigma_evidence_uses_selected_sigma() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];

        let block = BftBlock::try_from_with_dynamic_sigma_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 3),
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers.clone(),
        )
        .expect("valid evidence and matching selected sigma depth should be accepted");

        assert_eq!(block.headers, headers);
    }

    #[test]
    fn try_from_with_dynamic_sigma_evidence_rejects_below_required_sigma() {
        let err = BftBlock::try_from_with_dynamic_sigma_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 1),
            1,
            FatPointerToBftBlock2::null(),
            10,
            vec![test_header(BlockHash([0; 32]))],
        )
        .expect_err("selected sigma below controller floor should be rejected");

        assert!(matches!(
            err,
            InvalidDynamicSigmaBftBlock::DynamicSigmaEvidence(
                DynamicSigmaEvidenceError::SelectedSigmaBelowRequired {
                    selected: 1,
                    required: 3,
                }
            )
        ));
    }

    #[test]
    fn try_from_with_dynamic_sigma_evidence_requires_selected_header_depth() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];

        let err = BftBlock::try_from_with_dynamic_sigma_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 6),
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect_err("selected sigma 6 should require six headers");

        assert!(matches!(
            err,
            InvalidDynamicSigmaBftBlock::BftBlock(InvalidBftBlock::IncorrectConfirmationDepth {
                expected: 6,
                actual: 3,
            })
        ));
    }

    #[test]
    fn dynamic_sigma_bft_block_payload_constructs_and_serializes() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];
        let evidence = dynamic_sigma_evidence(63, 3);

        let payload = DynamicSigmaBftBlockPayload::try_from_with_evidence(
            dynamic_sigma_params(),
            evidence,
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect("valid dynamic-sigma evidence should build a payload");

        let encoded = payload
            .zcash_serialize_to_vec()
            .expect("payload serialization should succeed");
        let decoded = DynamicSigmaBftBlockPayload::zcash_deserialize(encoded.as_slice())
            .expect("payload deserialization should succeed");

        assert!(encoded.starts_with(&DYNAMIC_SIGMA_BFT_BLOCK_PAYLOAD_MAGIC));
        assert_eq!(
            decoded
                .zcash_serialize_to_vec()
                .expect("decoded payload serialization should succeed"),
            encoded,
        );
        assert_eq!(decoded.evidence, evidence);
        assert_eq!(decoded.block.headers.len(), 3);
    }

    #[test]
    fn dynamic_sigma_bft_block_payload_rejects_wrong_magic() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];
        let payload = DynamicSigmaBftBlockPayload::try_from_with_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 3),
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect("valid dynamic-sigma evidence should build a payload");
        let mut encoded = payload
            .zcash_serialize_to_vec()
            .expect("payload serialization should succeed");
        encoded[0] ^= 0xff;

        assert!(matches!(
            DynamicSigmaBftBlockPayload::zcash_deserialize(encoded.as_slice()),
            Err(SerializationError::Parse(
                "invalid dynamic sigma BFT payload magic"
            ))
        ));
    }

    #[test]
    fn dynamic_sigma_bft_block_payload_validates_constructed_payload() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];
        let payload = DynamicSigmaBftBlockPayload::try_from_with_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 3),
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect("valid dynamic-sigma evidence should build a payload");

        payload
            .validate(dynamic_sigma_params())
            .expect("constructed dynamic-sigma payload should validate");
    }

    #[test]
    fn dynamic_sigma_bft_block_payload_validation_rejects_evidence_block_mismatch() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];
        let mut payload = DynamicSigmaBftBlockPayload::try_from_with_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 3),
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect("valid dynamic-sigma evidence should build a payload");
        payload.block.version = 2;

        assert!(matches!(
            payload.validate(dynamic_sigma_params()),
            Err(InvalidDynamicSigmaBftBlock::PayloadBlockMismatch)
        ));
    }

    #[test]
    fn bft_block_payload_decodes_legacy_fixed_sigma_block() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];
        let block = BftBlock::try_from_with_confirmation_depth(
            3,
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect("matching fixed sigma depth should build a block");
        let encoded = block
            .zcash_serialize_to_vec()
            .expect("block serialization should succeed");

        let decoded = BftBlockPayload::zcash_deserialize_from_slice(encoded.as_slice())
            .expect("fixed-sigma payload should decode");

        assert!(matches!(decoded, BftBlockPayload::FixedSigma(_)));
        assert_eq!(
            decoded
                .block()
                .zcash_serialize_to_vec()
                .expect("decoded block serialization should succeed"),
            encoded,
        );
    }

    #[test]
    fn bft_block_payload_decodes_tagged_dynamic_sigma_payload() {
        let headers = vec![
            test_header(BlockHash([0; 32])),
            test_header(BlockHash([1; 32])),
            test_header(BlockHash([2; 32])),
        ];
        let payload = DynamicSigmaBftBlockPayload::try_from_with_evidence(
            dynamic_sigma_params(),
            dynamic_sigma_evidence(63, 3),
            1,
            FatPointerToBftBlock2::null(),
            10,
            headers,
        )
        .expect("valid dynamic-sigma evidence should build a payload");
        let encoded = payload
            .zcash_serialize_to_vec()
            .expect("payload serialization should succeed");

        let decoded = BftBlockPayload::zcash_deserialize_from_slice(encoded.as_slice())
            .expect("dynamic-sigma payload should decode");

        assert!(matches!(decoded, BftBlockPayload::DynamicSigma(_)));
        assert_eq!(
            decoded
                .block()
                .zcash_serialize_to_vec()
                .expect("decoded block serialization should succeed"),
            payload
                .block
                .zcash_serialize_to_vec()
                .expect("payload block serialization should succeed"),
        );
    }
}
