//! Shared types for the chronicle-registry LEZ program (LP-17 on-chain CID registry).
//!
//! Single-account design: all anchored records live in one PDA at
//! `[b"registry"]`. The on-chain key is the CID string itself.
//! Capacity is bounded by the 100 KiB account-data cap (~900 records
//! at typical CIDv1 base32 sizes).

use std::collections::HashMap;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Maximum records accepted in a single `index_batch` call.
/// Matches LP-17 R8's 50-CID batch benchmark size.
pub const MAX_BATCH: usize = 50;

/// One on-chain record per anchored CID.
///
/// Per LP-17, the registry stores `(CID, metadata_hash, anchor_timestamp)`
/// per document. The CID itself is the map key in `Registry.entries`;
/// we add `anchored_by` for audit and `version` for envelope evolution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct CidRecord {
    pub metadata_hash: [u8; 32],
    pub anchor_timestamp: i64,
    pub anchored_by: [u8; 32],
    pub version: u8,
}

/// Global registry state. Stored in the account at PDA `[b"registry"]`.
/// Borsh serializes `HashMap` with entries sorted by key, so on-chain bytes
/// are deterministic across guest executions.
#[derive(Debug, Clone, Default, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Registry {
    pub entries: HashMap<String, CidRecord>,
}

// ── Error codes (numbered, dist-x style) ─────────────────────────────────
// Used with `SpelError::custom(code, msg)` inside the guest program.

pub const E_INVALID_HASH: u32 = 1;     // cid is empty, or metadata_hash is all-zero
pub const E_BAD_TIMESTAMP: u32 = 2;    // anchor_timestamp == 0
pub const E_BATCH_EMPTY: u32 = 3;      // index_batch called with 0 records
pub const E_BATCH_TOO_BIG: u32 = 4;    // batch size > MAX_BATCH
pub const E_REGISTRY_FULL: u32 = 5;    // appending would exceed 100 KiB account cap
pub const E_ARITY_MISMATCH: u32 = 8;   // parallel vec lengths don't match
