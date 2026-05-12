//! C FFI for the chronicle_registry SPEL program.
//!
//! Modelled on logos-co/whisper-wall's `ui/ffi/src/lib.rs` (which is in turn
//! modelled on spel-client-gen output). Three IDL instructions exposed:
//!
//!   - `chronicle_registry_init_registry`  → idempotent init of the PDA
//!   - `chronicle_registry_index_batch`    → anchor 1..=MAX_BATCH CIDs
//!   - `chronicle_registry_get_registry`   → fetch + borsh-decode the PDA
//!                                           (manual; spel-client-gen issue #143)
//!
//! Every call accepts a JSON string with at least:
//!   { "wallet_path": "...", "sequencer_url": "...", "program_id_hex": "<64 hex>" }
//! and returns:
//!   { "ok": true,  ... }  // tx_hash on writes, entries on reads
//!   { "ok": false, "error": "..." }
//!
//! Returned strings are heap-allocated; the caller must free them with
//! `chronicle_registry_free_string`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use nssa::public_transaction::{Message, WitnessSet};
use nssa::{AccountId, ProgramId, PublicTransaction};
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

use chronicle_registry_core::{Registry, MAX_BATCH};

// ─────────────────────────────────────────────────────────────────────────────
// Instruction enum — must match the variant shape that spel-framework's
// `#[lez_program]` macro generates from chronicle-registry's guest functions.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChronicleRegistryInstruction {
    InitRegistry,
    IndexBatch {
        cids: Vec<String>,
        metadata_hashes: Vec<[u8; 32]>,
        anchor_timestamps: Vec<u32>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err("null pointer".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| format!("invalid UTF-8: {}", e))
}

fn to_cstring(s: String) -> *mut c_char {
    CString::new(s)
        .unwrap_or_else(|_| {
            CString::new(r#"{"ok":false,"error":"null byte in output"}"#).unwrap()
        })
        .into_raw()
}

fn error_json(msg: &str) -> *mut c_char {
    let v = serde_json::json!(msg).to_string();
    to_cstring(format!(r#"{{"ok":false,"error":{}}}"#, v))
}

fn parse_program_id_hex(s: &str) -> Result<ProgramId, String> {
    let s = s.trim_start_matches("0x");
    if s.len() != 64 {
        return Err(format!(
            "program_id_hex must be 64 hex chars, got {}",
            s.len()
        ));
    }
    let bytes = hex::decode(s).map_err(|e| format!("invalid hex: {}", e))?;
    let mut pid = [0u32; 8];
    for (i, chunk) in bytes.chunks(4).enumerate() {
        pid[i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    Ok(pid)
}

fn parse_account_id(s: &str) -> Result<AccountId, String> {
    // Tolerate base58-with-prefix forms ("Public/ABC123", "Private/ABC123")
    // as well as bare base58 strings.
    let base58 = s
        .strip_prefix("Public/")
        .or_else(|| s.strip_prefix("Private/"))
        .unwrap_or(s);
    base58
        .parse()
        .map_err(|_| format!("invalid AccountId: {}", s))
}

fn init_wallet(v: &Value) -> Result<WalletCore, String> {
    // WalletCore::from_env reads NSSA_WALLET_HOME_DIR + NSSA_SEQUENCER_URL.
    // Set them inline from the args so every FFI call is self-contained.
    if let Some(p) = v["wallet_path"].as_str() {
        std::env::set_var("NSSA_WALLET_HOME_DIR", p);
    }
    if let Some(u) = v["sequencer_url"].as_str() {
        std::env::set_var("NSSA_SEQUENCER_URL", u);
    }
    WalletCore::from_env().map_err(|e| format!("wallet init: {}", e))
}

fn compute_registry_pda(program_id: &ProgramId) -> AccountId {
    // Matches `pda = [literal("registry")]` in the guest program. The macro
    // zero-pads the seed to 32 bytes.
    let seed = nssa_core::program::PdaSeed::new({
        let mut b = [0u8; 32];
        b[..8].copy_from_slice(b"registry");
        b
    });
    // rc3 renamed the tuple-From impl that whisper-wall used (rc1) to a
    // named constructor on AccountId.
    AccountId::for_public_pda(program_id, &seed)
}

fn submit_tx(
    wallet: &WalletCore,
    program_id: ProgramId,
    account_ids: Vec<AccountId>,
    signer_ids: Vec<AccountId>,
    instruction: ChronicleRegistryInstruction,
) -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("tokio: {}", e))?;
    rt.block_on(async {
        let nonces = wallet
            .get_accounts_nonces(signer_ids.clone())
            .await
            .map_err(|e| format!("nonces: {}", e))?;
        let mut signing_keys = Vec::new();
        for sid in &signer_ids {
            let key = wallet
                .storage()
                .user_data
                .get_pub_account_signing_key(*sid)
                .ok_or_else(|| format!("signing key not found for {}", sid))?;
            signing_keys.push(key);
        }
        let message = Message::try_new(program_id, account_ids, nonces, instruction)
            .map_err(|e| format!("message: {:?}", e))?;
        let witness_set = WitnessSet::for_message(&message, &signing_keys);
        let tx = PublicTransaction::new(message, witness_set);
        wallet
            .sequencer_client
            .send_transaction(common::transaction::NSSATransaction::Public(tx))
            .await
            .map_err(|e| format!("submit: {}", e))
            .map(|r| hex::encode(r.0))
    })
}

fn ffi_call(
    f: impl FnOnce() -> Result<String, String> + std::panic::UnwindSafe,
) -> *mut c_char {
    match std::panic::catch_unwind(f) {
        Ok(Ok(r)) => to_cstring(r),
        Ok(Err(e)) => error_json(&e),
        Err(e) => {
            let msg = e
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("<unknown panic>");
            error_json(&format!("panic: {}", msg))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// init_registry
// ─────────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn chronicle_registry_init_registry(args_json: *const c_char) -> *mut c_char {
    let args = match cstr_to_str(args_json) {
        Ok(s) => s.to_owned(),
        Err(e) => return error_json(&e),
    };
    ffi_call(move || init_registry_impl(&args))
}

fn init_registry_impl(args: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(args).map_err(|e| format!("invalid JSON: {}", e))?;
    let program_id = parse_program_id_hex(
        v["program_id_hex"]
            .as_str()
            .ok_or("missing program_id_hex")?,
    )?;
    let wallet = init_wallet(&v)?;
    let anchorer = parse_account_id(v["anchorer"].as_str().ok_or("missing anchorer")?)?;
    let registry_pda = compute_registry_pda(&program_id);
    let tx_hash = submit_tx(
        &wallet,
        program_id,
        vec![registry_pda, anchorer],
        vec![anchorer],
        ChronicleRegistryInstruction::InitRegistry,
    )?;
    Ok(json!({"ok": true, "tx_hash": tx_hash}).to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// index_batch
// ─────────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn chronicle_registry_index_batch(args_json: *const c_char) -> *mut c_char {
    let args = match cstr_to_str(args_json) {
        Ok(s) => s.to_owned(),
        Err(e) => return error_json(&e),
    };
    ffi_call(move || index_batch_impl(&args))
}

fn index_batch_impl(args: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(args).map_err(|e| format!("invalid JSON: {}", e))?;
    let program_id = parse_program_id_hex(
        v["program_id_hex"]
            .as_str()
            .ok_or("missing program_id_hex")?,
    )?;
    let wallet = init_wallet(&v)?;
    let anchorer = parse_account_id(v["anchorer"].as_str().ok_or("missing anchorer")?)?;

    let entries = v["entries"].as_array().ok_or("missing entries array")?;
    if entries.is_empty() {
        return Err("entries: empty batch".into());
    }
    if entries.len() > MAX_BATCH {
        return Err(format!(
            "entries: {} exceeds MAX_BATCH={}",
            entries.len(),
            MAX_BATCH
        ));
    }

    let mut cids = Vec::with_capacity(entries.len());
    let mut metadata_hashes = Vec::with_capacity(entries.len());
    let mut anchor_timestamps = Vec::with_capacity(entries.len());

    for (i, e) in entries.iter().enumerate() {
        let cid = e["cid"]
            .as_str()
            .ok_or_else(|| format!("entries[{}].cid missing", i))?
            .to_string();
        if cid.is_empty() {
            return Err(format!("entries[{}].cid empty", i));
        }

        let hash_str = e["metadata_hash"]
            .as_str()
            .ok_or_else(|| format!("entries[{}].metadata_hash missing", i))?;
        let hash_bytes = hex::decode(hash_str.trim_start_matches("0x"))
            .map_err(|err| format!("entries[{}].metadata_hash invalid hex: {}", i, err))?;
        let hash_arr: [u8; 32] = hash_bytes.as_slice().try_into().map_err(|_| {
            format!(
                "entries[{}].metadata_hash must be 32 bytes (got {})",
                i,
                hash_bytes.len()
            )
        })?;

        let ts = e["timestamp"]
            .as_u64()
            .or_else(|| e["timestamp"].as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| format!("entries[{}].timestamp missing or not numeric", i))?;
        if ts == 0 {
            return Err(format!("entries[{}].timestamp == 0", i));
        }
        if ts > u32::MAX as u64 {
            return Err(format!(
                "entries[{}].timestamp {} exceeds u32 (year 2106 cap)",
                i, ts
            ));
        }

        cids.push(cid);
        metadata_hashes.push(hash_arr);
        anchor_timestamps.push(ts as u32);
    }

    let registry_pda = compute_registry_pda(&program_id);
    let tx_hash = submit_tx(
        &wallet,
        program_id,
        vec![registry_pda, anchorer],
        vec![anchorer],
        ChronicleRegistryInstruction::IndexBatch {
            cids,
            metadata_hashes,
            anchor_timestamps,
        },
    )?;
    Ok(json!({"ok": true, "tx_hash": tx_hash}).to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// get_registry — fetches + borsh-decodes the registry PDA without a tx.
// spel-client-gen doesn't emit a read path yet (see whisper-wall NOTES.md §
// spel-client-gen #143); written manually here.
// ─────────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn chronicle_registry_get_registry(args_json: *const c_char) -> *mut c_char {
    let args = match cstr_to_str(args_json) {
        Ok(s) => s.to_owned(),
        Err(e) => return error_json(&e),
    };
    ffi_call(move || get_registry_impl(&args))
}

fn get_registry_impl(args: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(args).map_err(|e| format!("invalid JSON: {}", e))?;
    let program_id = parse_program_id_hex(
        v["program_id_hex"]
            .as_str()
            .ok_or("missing program_id_hex")?,
    )?;
    let wallet = init_wallet(&v)?;
    let registry_pda = compute_registry_pda(&program_id);

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("tokio: {}", e))?;
    let registry: Registry = rt.block_on(async {
        let account = wallet
            .sequencer_client
            .get_account(registry_pda)
            .await
            .map_err(|e| format!("get_account: {}", e))?;
        if account.data.is_empty() {
            return Ok::<Registry, String>(Registry::default());
        }
        Registry::try_from_slice(&account.data).map_err(|e| format!("borsh decode: {}", e))
    })?;

    let mut entries = serde_json::Map::new();
    for (cid, rec) in registry.entries {
        entries.insert(
            cid,
            json!({
                "metadata_hash":    hex::encode(rec.metadata_hash),
                "anchor_timestamp": rec.anchor_timestamp,
                "anchored_by":      hex::encode(rec.anchored_by),
                "version":          rec.version,
            }),
        );
    }
    Ok(json!({"ok": true, "entries": entries}).to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility
// ─────────────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn chronicle_registry_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

#[no_mangle]
pub extern "C" fn chronicle_registry_version() -> *mut c_char {
    to_cstring("0.1.0".to_string())
}
