# Chronicle on-chain anchoring

How chronicle commits published `(cid, metadata_hash, anchor_timestamp)`
tuples to the chronicle-registry SPEL program, and how to test the path
end-to-end from the CLI.

---

## What's built

A pipeline that lets chronicle submit a real on-chain transaction anchoring
a batch of CIDs into the chronicle-registry account. Two repos, two pieces.

### 1. `chronicle-registry/ffi/` — Rust cdylib

Small Rust crate (`chronicle_registry_ffi`) modelled on whisper-wall's
`ui/ffi/`. Builds to `libchronicle_registry_ffi.so`. Exposes five
`extern "C"` symbols:

```
chronicle_registry_index_batch     — submits an index_batch transaction (UI path)
chronicle_registry_init_registry   — one-time PDA init; exposed for smoke setup
chronicle_registry_get_registry    — reads the PDA + borsh-decodes it; smoke
                                     uses this to prove the tx actually applied
chronicle_registry_free_string     — frees strings returned by the above
chronicle_registry_version
```

Wire format is JSON in / JSON out. Each call accepts
`{program_id_hex, wallet_path, sequencer_url, ...args}` and returns
`{ok: true, tx_hash: "..."}` on success or `{ok: false, error: "..."}` on
failure. The crate links `nssa`, `wallet`, `sequencer_service_rpc` at LEZ
tag `v0.2.0-rc3` (matching the guest program) and uses
`WalletCore::from_env()` to pick up `NSSA_WALLET_HOME_DIR` +
`NSSA_SEQUENCER_URL`, set inline from the JSON args per call.

The crate has its own `[workspace]` declaration so the heavy LEZ dep tree
(zk circuits, RISC-V VM, etc.) doesn't bleed into chronicle-registry's
parent workspace; it's listed in the parent's `exclude` list.

### 2. `scaffold-next/logos-chronicle/` — C++ glue

- **`chronicle_anchor_client.{h,cpp}`** — a thin Qt class that dynamically
  loads the `.so` via `QLibrary`. Resolves all five symbols on first use,
  fails gracefully if the library isn't present. Library path comes from
  the `CHRONICLE_REGISTRY_FFI_PATH` env var; otherwise QLibrary's standard
  search (LD_LIBRARY_PATH, plugin's own dir if rpath is set, etc.).
- **`chronicle_plugin.cpp::anchorBatchJson`** — previously a stub returning
  `ANCHOR_NOT_IMPLEMENTED`; now reads the persisted anchor config, builds
  the FFI request from the caller's entries, blocks on `indexBatch` (the
  FFI's internal Tokio runtime handles the actual async), then writes a
  terminal record per CID into the local anchor ledger — `confirmed` with
  the real `tx_hash` on success, `failed` with the FFI error otherwise.
- **Default signer** is `CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r` (the
  first public account in `chronicle-registry/.scaffold/wallet/wallet_config.json`).
  The earlier default from `batch-anchor.toml` wasn't actually in this wallet.

---

## How we test it

`scripts/logoscore-anchor-smoke.sh` — pure-CLI, no UI involved.

### Preconditions

The script checks all of these and exits with a clear message if any are
missing.

1. **FFI built**: `chronicle-registry/ffi/target/release/libchronicle_registry_ffi.so`
   exists. Built via:
   ```bash
   cd chronicle-registry/ffi && cargo build --release
   ```
2. **Chronicle installed for logoscore**:
   `/tmp/chronicle-next-install/modules/chronicle/` exists. Built via:
   ```bash
   cd scaffold-next/logos-chronicle && nix build path:.#install --out-link /tmp/chronicle-next-install
   ```
3. **Sequencer reachable** at `127.0.0.1:3040`. Started via:
   ```bash
   cd chronicle-registry && lgs localnet start
   ```
4. **Wallet directory** exists at `chronicle-registry/.scaffold/wallet/`.
5. **Deployed program_id** in env (defaults to the LP-17 dev deployment
   pinned in the script).

### What the script does

1. Creates a per-run `RUN_DIR` so it doesn't see anchor records from prior
   runs, with the same `EXIT/INT/TERM` trap + `pkill -f $RUN_DIR` fallback
   the other chronicle smoke scripts use.
2. Boots a `logoscore -D` daemon with `CHRONICLE_REGISTRY_FFI_PATH` pointing
   at the built `.so`, loads the chronicle module.
3. `setAnchorConfigJson` — writes the four config fields (program_id,
   sequencer_url, wallet_home, signer_account_id) and asserts
   `configured == true`.
4. `getRegistryJson` (precondition probe) — if the PDA is unreachable
   (sequencer-side `get_account` error), the script runs `initRegistryJson`
   once. The guest's `#[account(init, …)]` errors on a second init, so we
   only init when we have to. Confirms the PDA is reachable before anchoring.
5. `anchorBatchJson` — submits a one-entry batch with a synthetic CID
   (`zSmoke-<ts>-<rand>`), random 32-byte hash, current unix timestamp.
   Inside chronicle this hits the FFI → nssa builds a `Message` → wallet
   signs → sequencer accepts. Asserts the response has `ok: true` and a
   non-empty `tx_hash`.
6. `listAnchorsJson` — reads chronicle's local ledger map and asserts the
   CID is present with `state == "confirmed"` and the matching `tx_hash`.
   Proves local persistence works.
7. `getRegistryJson` (post-anchor) — reads the PDA back, borsh-decodes
   the `Registry`, asserts our CID is in `entries` and its `metadata_hash`
   matches what we submitted. **This is the proof the tx actually applied
   on-chain, not just that the sequencer accepted it.**
8. `lookupAnchorJson <cid>` — asserts `found == true`. Proves the local
   cache is consulted before any chain hit.

### Sample successful run

```
config saved
anchoring cid=zSmoke-1778546682-f2f5d628 timestamp=1778546682
tx_hash=02a84f7b4e06c822848470a0fd21b136dcd9b1ac889a905088fd7d4c1e32ac9a
persisted: state=confirmed tx_hash=02a84f7b4e06c822...
lookup: found
ok cid=zSmoke-1778546682-f2f5d628 tx_hash=02a84f… run_dir=/tmp/chronicle-anchor-smoke-1778546681
```

The `tx_hash` is a real on-chain commit; verifiable against the sequencer.

### To re-run

```bash
scaffold-next/logos-chronicle/scripts/logoscore-anchor-smoke.sh
```

Override any defaults via env vars: `ANCHOR_PROGRAM_ID`,
`ANCHOR_SIGNER_ACCOUNT`, `ANCHOR_SEQUENCER_URL`, `ANCHOR_WALLET_HOME`,
`FFI_LIB`.

---

## What's NOT tested here

The UI path. The UI calls the same `anchorBatchJson` underneath, but
verifying the QML button states + per-row "Anchored ✓" / "Retry" badges
under real on-chain timing is the next session's work. The synchronous
nature of chronicle's current dispatch (1–3s per tx) means the QtRemoteObjects
worker thread stalls during the call; replacing this with `QtConcurrent::run`
+ `QFutureWatcher` (whisper-wall's pattern) is the planned phase 3.
