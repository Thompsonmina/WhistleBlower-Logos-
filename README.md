# Whistleblower

A censorship-resistant document upload + indexing pipeline for the Logos
stack — the reference implementation of [LP-17](../LP_17).  Anyone can
upload a document, have its content-identifier (CID) broadcast peer-to-peer
in real time, and optionally anchor that CID on-chain so it remains
discoverable indefinitely without a trusted host.

> **Why this exists.**  Whistleblowers, journalists, and activists need
> publication tools that survive takedowns.  Existing options either
> rely on a single trusted host, require the publisher to hold tokens,
> or provide no long-term discoverability guarantee.  Whistleblower
> stitches together three Logos primitives — Storage, Delivery, and the
> LEZ (Logos Execution Zone) — into a complete publish-and-anchor flow,
> and extracts the moving parts into a reusable module any other
> Basecamp app can drop in.

## How the pieces fit together

```
┌─────────────────┐   upload   ┌──────────────┐
│  Basecamp app   │ ──────────▶│ Logos Storage│
│ (logos-         │            │   (Codex)    │
│  whistleblower) │            └──────┬───────┘
└────────┬────────┘                   │  CID
         │ broadcast(CID,metadata)    │
         ▼                            ▼
┌─────────────────┐ ◀───── subscribe ─────────┐
│  Logos Delivery │                            │
│     (Waku)      │                            │
└────────┬────────┘                            │
         │  envelope                           │
         ▼                                     │
┌─────────────────┐    index_batch     ┌──────┴──────────┐
│   batch-anchor  │ ────────────────▶  │ chronicle-      │
│      CLI        │  (50 CIDs/tx)      │ registry (LEZ)  │
└─────────────────┘                    └─────────────────┘
```

The publish path on the left is owned by [`logos-chronicle`](logos-chronicle/README.md)
— a reusable Basecamp module that wraps Storage + Delivery.  The
anchoring path on the right is owned by [`chronicle-registry`](README_CHRONICLE_REGISTRY.md)
(the SPEL program) and [`batch-anchor`](batch-anchor/README.md) (the
permissionless CLI that aggregates broadcast CIDs and commits them
on-chain in batches).  Two separate paths, two separate consumers,
one shared on-chain registry.

## Components

Components with their own README:

| Path | Purpose |
|---|---|
| **[logos-whistleblower/](logos-whistleblower/README.md)** | The Basecamp app GUI.  Lets a user pick a file, attach metadata, and submit. |
| **[logos-chronicle/](logos-chronicle/README.md)** | The reusable document-indexing module: upload → broadcast → (optionally) anchor.  Has its own `chronicle_plugin` and Qt Remote Objects interface so any Basecamp app can depend on it. |
| **[chronicle-registry](README_CHRONICLE_REGISTRY.md)** *(root)* | The on-chain CID registry — a SPEL program for LEZ.  Stores `(CID, metadata_hash, anchor_timestamp)` keyed on CID.  Permissionless, idempotent, batches up to 50 entries per tx.  The README also covers the registry's internals: `methods/` (RISC0 guest), `chronicle_registry_core/` (shared types), `examples/` (auto-generated CLI + IDL generator). |
| **[batch-anchor/](batch-anchor/README.md)** | The permissionless batch-anchor daemon.  Subscribes to a Waku topic, accumulates CIDs, and submits `index_batch` transactions in batches of up to 50. |

Supporting directories without their own README (cross-referenced from the ones above):

- `methods/` — RISC0 guest binary source; see [chronicle-registry README § Project layout](README_CHRONICLE_REGISTRY.md).
- `chronicle_registry_core/` — shared on-chain/off-chain types; same.
- `examples/` — auto-generated `chronicle_registry_cli` + `generate_idl` bins; same.
- `ffi/` — C ABI cdylib that `logos-chronicle` links against for LEZ calls.
- `scripts/` — bootstrap scripts (`setup.sh` builds guest + deploys + mints signer if needed + opens registry).

## LP-17 requirement map

Each row points to where the work lives.  Status reflects what's
landed in this repo; pending rows are tracked in [/planning](../planning).

| # | Requirement | Implementation |
|---|---|---|
| F1 | Upload to Logos Storage → CID | [logos-chronicle](logos-chronicle/README.md) (storage publish path) |
| F2 | Broadcast metadata envelope to Logos Delivery | [logos-chronicle](logos-chronicle/README.md) (delivery publish path) + [envelope schema](docs/metadata-envelope.md) |
| F3 | UI "Anchor on-chain" action | [logos-chronicle/docs/anchoring.md](logos-chronicle/docs/anchoring.md) — chronicle's `anchorBatchJson` / `anchorStatusJson` / `lookupAnchorJson` methods + Basecamp UI wiring |
| F4 | Batch anchor CLI (permissionless, idempotent) | [batch-anchor](batch-anchor/README.md) |
| F5a | On-chain registry (LEZ SPEL program) | [chronicle-registry](README_CHRONICLE_REGISTRY.md) (guest source under `methods/guest/`) |
| F5b | LEZ-vs-zone-SDK justification | [see below](#anchoring-approach--why-lez-over-zone-sdk-f5b) |
| F6 | Document-indexing module (extracted, reusable) | [logos-chronicle](logos-chronicle/README.md) |
| U7 | Basecamp app GUI loadable in Basecamp | [logos-whistleblower](logos-whistleblower/README.md) |
| U8 | Module SDK README / API doc | [logos-chronicle/README.md](logos-chronicle/README.md) + [logos-chronicle/docs/api.md](logos-chronicle/docs/api.md) |
| U9 | IDL for the LEZ program | [`chronicle-registry-idl.json`](chronicle-registry-idl.json), regen via `make idl` ([examples/](examples/README.md)) |
| R10 | Upload retries with exponential backoff | [logos-chronicle/docs/api.md](logos-chronicle/docs/api.md) (storage publish path) |
| R11 | Broadcast dedup | [logos-chronicle/docs/api.md](logos-chronicle/docs/api.md) (delivery layer) |
| R12 | Batch tool resume after interruption | [batch-anchor/README.md § Reliability story (LP-17 R3)](batch-anchor/README.md) |
| P13 | CU benchmarks (1-CID + 50-CID) | [chronicle-registry § Compute units](README_CHRONICLE_REGISTRY.md) |
| S15 | E2E integration tests in CI | _pending_ — IT contract sketched in [`integration-test.toml`](integration-test.toml) |
| S17 | Top-level README | this file |
| S18 | Reproducible E2E demo script with `RISC0_DEV_MODE=0` | [scripts/run-app.sh](scripts/run-app.sh) + chronicle smokes on the IT topic — [see below](#e2e-demo-s18) |
| SUB | MIT + Apache-2.0 license files | [LICENSE-MIT](LICENSE-MIT) + [LICENSE-APACHE](LICENSE-APACHE) |

### Anchoring approach — why LEZ over zone-SDK (F5b)

LP-17 lets the submitter pick between a LEZ program or a direct zone-SDK
consensus inscription, and asks for a brief justification.  We picked
the **LEZ SPEL program** for two reasons:

1. **It matches the spirit of the tool.**  LP-17 itself notes that the
   zone-SDK path "requires a single designated actor to perform
   consensus inscription, which affects the trust model."  A SPEL program on LEZ is permissionless: anyone with a
   wallet can submit
2. **Established tooling is much further along.**  SPEL ships IDL
   generation, scaffold templates, an auto-generated CLI, and the
   `#[lez_program]` macro that handles account-claim plumbing.  We
   write the program logic, the framework handles the wire format.
   Going via the zone-SDK would mean implementing the inscription
   format, signature handling, and submission path by hand — strictly
   more code for less decentralisation.


## Quick setup

**Fast path:** once the prerequisites below are installed,
`./scripts/run-app.sh` walks the whole flow — builds the registry guest,
deploys it, mints/checks the signer, opens the registry PDA, brings up
nwaku + the batch-anchor watcher, and finally launches Basecamp with
our modules loaded. The step-by-step that follows is what `run-app.sh`
does on your behalf, broken out in case you want to drive a single
piece manually.

### Prerequisites

- Rust + [risc0 toolchain](https://dev.risczero.com/api/zkvm/install)
- [logos-scaffold](https://github.com/logos-co/logos-scaffold) (`lgs`)
- Docker (for nwaku)
- A local Codex/LogosCore node (for storage retrieval; only needed if
  you also want to fetch the document bytes back)

### Steps

1. **Start a LEZ sequencer** — `lgs localnet start`.  Hosts the chain
   at `127.0.0.1:3040` and creates `.scaffold/wallet/` if missing.

2. **Bootstrap the registry side** — `scripts/setup.sh`.  Idempotent.
   Builds the guest if needed (~3 min cold), deploys it to the
   sequencer, ensures the signer pinned in
   [batch-anchor/batch-anchor.toml](batch-anchor/batch-anchor.toml) is
   in the wallet (mints + rewrites the toml if not), and calls
   `batch-anchor init` to open the registry PDA.

3. **Start a Waku node** — `cd batch-anchor && ./target/debug/batch-anchor node up`.
   Brings up a single-node nwaku (cluster 2, 8 shards, Logos Dev fleet
   staticnodes) on `127.0.0.1:8645`.

4. **Start the anchor** — `./target/debug/batch-anchor watch`.
   Subscribes to `/chronicle/1/document-index/json`, accumulates CIDs,
   anchors them to the registry in batches.

5. **(Optional) Install the Basecamp app** — see
   [logos-whistleblower/README.md](logos-whistleblower/README.md) for
   loading the UI into Basecamp.

All of steps 2–5 above are bundled in `./scripts/run-app.sh` — clone
the repo, start a sequencer, and one command takes it from there.

### E2E smoke tests (S18)

Each end-to-end stage of the pipeline has a nix-wrapped smoke that runs
against the **locked** module revisions in `flake.lock`. Every dependency
(logoscore CLI, storage_module, delivery_module, chronicle) is resolved
from a flake input — cloners do not need to set env vars or pre-install
anything else.


A clone-and-run path that drives the whole pipeline (upload →
broadcast → anchor → on-chain lookup) without touching "production"
traffic.  Two pieces work together via the shared
[`integration-test.toml`](integration-test.toml) topic contract:

1. **Boot the sequencer and bootstrap the registry.**
   ```bash
   lgs localnet start
   scripts/setup.sh
   ```
   `setup.sh` builds the guest if needed, deploys the program
   (deterministic — same `program_id` on every chain), mints a signer
   on the test wallet if the pinned one isn't already present, and
   opens the registry PDA.

2. **Start the batch-anchor against the integration topic.**
   ```bash
   cd batch-anchor
   ./target/debug/batch-anchor -c batch-anchor.it.toml watch
   ```
   The `.it.toml` config differs from the production one in exactly
   one place: `content_topic = "/chronicle/it/document-index/json"`.
   That topic-only isolation means a production anchor on
   `/chronicle/1/…` won't see the demo traffic, and the demo anchor
   won't accidentally anchor any real broadcasts.

```bash
# Storage upload only (storage_module → CID)
nix run .#smoke-storage

# Build envelope + broadcast it on Logos Delivery (uses the IT topic
# from integration-test.toml so test envelopes never reach production
# /chronicle/1/... subscribers)
nix run .#smoke-broadcast

# Full publish path: storage upload + broadcast, restart-persistence,
# delivery propagation verified in the daemon log
nix run .#smoke-publish

# On-chain anchor flow — see § Anchor smoke below
nix run .#smoke-anchor
```

Each smoke runs in its own ephemeral `RUN_DIR=/tmp/chronicle-<smoke>-smoke-<ts>/`,
boots a logoscore daemon scoped to that dir, and reaps every spawned
host process on exit (EXIT/INT/TERM traps + `pkill` on the run-dir
substring). Concurrent runs and re-runs do not collide.

### What each one verifies

| Smoke | Asserts |
|---|---|
| `smoke-storage` | `uploadFileJson` returns a CID; `uploadStatusJson` reports `ok` with that CID. |
| `smoke-broadcast` | `buildMetadataEnvelopeJson` produces a well-formed envelope; `broadcastEnvelopeJson` reaches `status=sent`. Topic comes from `[topic].content` in `integration-test.toml` via a `chronicle.setBroadcastTopic` call at the top of the run. |
| `smoke-publish` | End-to-end publish: `publishFileJson` → poll `publishStatusJson` until `broadcast_sent`; verifies the `cid`, `metadata_hash`, `envelope`, and `broadcast_id` all land in the terminal status; asserts the **original filename never appears** in Storage's `Stored data` log (the staged title is what gets stored); confirms `Message successfully propagated`/`sent` lines in delivery's stderr; restarts logoscore and asserts the publish ledger reloads from disk. |
| `smoke-anchor` | End-to-end on-chain: `setAnchorConfigJson` saves the config and reports `configured=true`; if the registry PDA doesn't exist yet (fresh sequencer), `initRegistryJson` opens it; `anchorBatchJson` submits a synthetic `(cid, metadata_hash, anchor_timestamp)` and returns a real `tx_hash`; `listAnchorsJson` shows the record as `confirmed`; the script then polls `getRegistryJson` until the CID lands on-chain (sequencer batches ~30 s) and asserts `metadata_hash` + `anchor_timestamp` match; finally `lookupAnchorJson` confirms by CID. |

### Config

Smokes read two fields from `integration-test.toml` at the repo root:

| Field | Used by | Override |
|---|---|---|
| `[topic].content` | broadcast, publish | `IT_TOPIC=...` env var |
| `[registry].program_id` | anchor | `IT_PROGRAM_ID=...` env var |

Everything else (sequencer URL, wallet, signer) is shared with the user's
production setup — no separate test wiring needed. The loader is at
`logos-chronicle/scripts/lib/load-integration-config.sh` if you want to
read or extend it.

### Anchor smoke

The anchor smoke is the only smoke that talks to a real sequencer, so it
needs the Quick-setup steps already done:

1. **Sequencer running** at `127.0.0.1:3040` (default) — `lgs localnet start`.
2. **Registry deployed + signer minted** — `scripts/setup.sh`.
   This is the same idempotent bootstrap step 2 of Quick setup. It
   builds the guest if needed, deploys chronicle-registry, ensures the
   signer pinned in `batch-anchor.toml` exists in `.scaffold/wallet`
   (mints + rewrites the toml if not), and opens the registry PDA.

Once those are done, run the smoke with your wallet's signer account:

```bash
ANCHOR_SIGNER_ACCOUNT=<base58 account_id from .scaffold/wallet> \
  nix run .#smoke-anchor
```

`ANCHOR_SIGNER_ACCOUNT` is the only knob with no static default — pick
any `account_id` from `.scaffold/wallet/storage.json` that has a signing
key (you can also grep `batch-anchor.toml`'s `signer_account_id` for the
one `setup.sh` provisioned). The defaults below cover everything else;
override any of them via env var if your setup differs:

| Env var | Default |
|---|---|
| `ANCHOR_PROGRAM_ID` | `[registry].program_id` from `integration-test.toml` |
| `ANCHOR_SEQUENCER_URL` | `http://127.0.0.1:3040` |
| `ANCHOR_WALLET_HOME` | `$REPO_ROOT/.scaffold/wallet` |
| `FFI_LIB` | unset — chronicle resolves the bundled `.so` from its plugin-dir at runtime |

The smoke is idempotent: re-running anchors a fresh synthetic CID each
time; the registry PDA is opened only on the first run (subsequent runs
detect it via `getRegistryJson` and skip the `initRegistryJson` call).

## Layout

```
whistleblower/                                # workspace root
├── README.md                                 # ← you are here
├── README_CHRONICLE_REGISTRY.md              # chronicle-registry program README
├── Makefile                                  # build / idl / cli / deploy targets
├── Cargo.toml                                # workspace manifest
├── chronicle-registry-idl.json               # generated IDL
├── spel.toml                                 # spel CLI config (binary + idl)
├── scaffold.toml                             # lgs scaffold pin
├── integration-test.toml                     # IT topic + program_id contract
│
├── methods/         (RISC0 guest binary)
├── chronicle_registry_core/                  (shared types)
├── examples/                                 (auto-generated CLI)
├── ffi/                                      (C ABI cdylib)
├── batch-anchor/                             (permissionless anchor daemon)
├── logos-chronicle/                          (reusable document-indexing module)
├── logos-whistleblower/                      (Basecamp app UI)
└── scripts/                                  (bootstrap scripts)
```

## License

Dual-licensed under either of:

- [MIT license](LICENSE-MIT) ([https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT))
- [Apache License, Version 2.0](LICENSE-APACHE) ([https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0))

at your option.  Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this work, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
