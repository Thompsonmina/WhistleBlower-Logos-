# chronicle-registry

On-chain CID registry for LP-17 (Whistleblower / censorship-resistant document
indexing). Implemented as a SPEL program on LEZ.

## What it is

A single permissionless registry PDA at `[b"registry"]` that holds anchored
`(CID, metadata_hash, anchor_timestamp)` records for the LP-17 indexing
pipeline. Anyone can call `init_registry` once, then anchor up to
`MAX_BATCH = 50` CIDs per `index_batch` transaction. Duplicates are silently
skipped — idempotency is enforced in-program so callers can replay batches
without coordination.

State layout:

```rust
pub struct Registry {
    pub entries: HashMap<String, CidRecord>,   // key = CID string
}

pub struct CidRecord {
    pub metadata_hash:    [u8; 32],
    pub anchor_timestamp: i64,                 // unix seconds
    pub anchored_by:      [u8; 32],            // signer account_id
    pub version:          u8,                  // envelope version (1)
}
```

Borsh serialises `HashMap` with entries sorted by key, so on-chain bytes are
deterministic across guest executions — required for risc0 proof determinism.

## LP-17 mapping

| Requirement | Where it lives                                                  |
|-------------|-----------------------------------------------------------------|
| R1 — tuple per document        | `CidRecord` fields                            |
| R2 — queryable by CID          | Map key is the CID string itself              |
| R3 — ≥10 CIDs per tx           | `MAX_BATCH = 50`                              |
| R4 — idempotent duplicates     | `entries.contains_key(...)` → silent skip    |
| R5 — permissionless            | No `Pubkey` allow-list; signer can be anyone  |
| R6 — LEZ program               | `#[lez_program]` guest binary                 |
| R7 — devnet deploy             | `make deploy`                                 |
| R8 — CU benchmarks             | (pending)                                     |
| R9 — CI tests                  | `chronicle_registry_core` unit tests + CLI smokes |

## Capacity

State is bounded by the 100 KiB per-account data cap. Per-record cost in Borsh:

| Field            | Bytes |
|------------------|-------|
| key length (u32) | 4     |
| CID string       | ~50   |
| metadata_hash    | 32    |
| anchor_timestamp | 8     |
| anchored_by      | 32    |
| version          | 1     |
| **Total**        | ~127  |

So ~800 CIDs fit in one registry account at typical CIDv1 sizes. When that
ceiling is approached, the recommended upgrade path is to add a second
generation of registries (`[b"registry", version_u8]`) and have readers
fall back to the previous generation if a CID isn't found in the current one.

## Instructions

### `init_registry(anchorer)`

Opens the registry PDA. Permissionless — anyone can call this once.
Re-init fails with `AccountAlreadyInitialized` (code 1002), which callers
should treat as a no-op success.

```bash
make cli ARGS="init-registry --anchorer <PUBLIC_ID>"
```

### `index_batch(anchorer, cids, metadata_hashes, anchor_timestamps)`

Three parallel vectors of equal length:

| Param               | CLI flag              | Format                                          |
|---------------------|-----------------------|-------------------------------------------------|
| `cids`              | `--cids`              | Comma-separated CIDs (no commas inside any CID) |
| `metadata_hashes`   | `--metadata-hashes`   | Comma-separated 32-byte hex strings             |
| `anchor_timestamps` | `--anchor-timestamps` | Comma-separated u32 unix-seconds                |

```bash
make cli ARGS="index-batch \
  --cids bafy1,bafy2,bafy3 \
  --metadata-hashes 0xaaaa...,0xbbbb...,0xcccc... \
  --anchor-timestamps 1715000000,1715000001,1715000002 \
  --anchorer <PUBLIC_ID>"
```

The `Vec<String>` arg type requires spel-cli ≥ the
`feat(cli): parse and serialize Vec<String> args from CSV input` patch.
Until merged upstream this repo pins
`scaffold.toml` at the [Thompsonmina/spel cli-vec-string branch](https://github.com/Thompsonmina/spel/tree/cli-vec-string).

#### Error codes

| Code | Constant            | Meaning                                            |
|------|---------------------|----------------------------------------------------|
| 1    | `E_INVALID_HASH`    | CID empty or `metadata_hash` all-zero              |
| 2    | `E_BAD_TIMESTAMP`   | `anchor_timestamp == 0`                            |
| 3    | `E_BATCH_EMPTY`     | Empty batch                                        |
| 4    | `E_BATCH_TOO_BIG`   | `n > MAX_BATCH`                                    |
| 5    | `E_REGISTRY_FULL`   | Appending would exceed the 100 KiB account cap     |
| 8    | `E_ARITY_MISMATCH`  | The three parallel vecs have different lengths     |
| 1002 | `AccountAlreadyInitialized` | (re-init only; treat as no-op success)     |

## Quick start

```bash
make build idl deploy setup           # build, generate IDL, deploy, mint signer
SIGNER=$(grep SIGNER_ID .chronicle_registry-state | cut -d= -f2)
export NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet"

make cli ARGS="init-registry --anchorer $SIGNER"
make cli ARGS="index-batch \
  --cids bafybeiTestCid001 \
  --metadata-hashes 0a11111111111111111111111111111111111111111111111111111111111100 \
  --anchor-timestamps $(date +%s) \
  --anchorer $SIGNER"
```

## Inspecting state

`lgs wallet account get` returns raw account bytes; the inline Python decoder
below walks the Borsh layout:

```bash
PDA=$(make cli ARGS="init-registry --anchorer $SIGNER --dry-run=text" \
        2>&1 | grep "PDA registry" | awk '{print $4}')

python3 - <<EOF
import json, subprocess
out = subprocess.check_output(['lgs','wallet','--','account','get','--account-id',
    f"Public/$PDA",'--raw'], text=True)
hexdata = json.loads([l for l in out.splitlines() if l.startswith('{')][0])['data']
b = bytes.fromhex(hexdata); i = 0
n = int.from_bytes(b[i:i+4],'little'); i+=4
print(f"entries: {n}")
for k in range(n):
    klen = int.from_bytes(b[i:i+4],'little'); i+=4
    cid  = b[i:i+klen].decode(); i+=klen
    mh   = b[i:i+32].hex(); i+=32
    ts   = int.from_bytes(b[i:i+8],'little', signed=True); i+=8
    by   = b[i:i+32].hex(); i+=32
    ver  = b[i]; i+=1
    print(f"  [{k}] cid={cid}  mh=0x{mh[:16]}…  ts={ts}  v{ver}")
EOF
```

## Project layout

```
chronicle-registry/
├── chronicle_registry_core/      # Shared types (Registry, CidRecord, error codes)
│   └── src/lib.rs
├── methods/guest/                # RISC0 guest binary
│   └── src/bin/chronicle_registry.rs
├── examples/                     # auto-generated CLI + IDL generator
│   └── src/bin/
│       ├── generate_idl.rs
│       └── chronicle_registry_cli.rs
├── batch-anchor/                 # standalone CLI: subscribes to Logos Delivery
│   └── ...                       #   and batch-anchors broadcast CIDs (see batch-anchor/README.md)
├── spel.toml                     # spel CLI config (IDL + binary paths)
├── scaffold.toml                 # logos-scaffold pins (LEZ + spel)
├── Makefile
└── chronicle-registry-idl.json   # auto-generated IDL
```

## Batch anchor tool

The sibling [batch-anchor/](batch-anchor/) directory holds the LP-17 batch
anchor CLI — a tokio daemon that subscribes to a Logos Delivery topic,
accumulates broadcast CIDs in memory, and submits them to this registry's
`index_batch` instruction in batches of up to 50. It is excluded from this
workspace (`exclude = ["batch-anchor"]`) so its tokio/reqwest dependencies
stay out of the risc0 guest build. See [batch-anchor/README.md](batch-anchor/README.md)
for build, configuration, and usage.

## Make targets

| Target | Description |
|--------|-------------|
| `make build` | Build the guest binary (risc0 docker build, ~2-3 min cold) |
| `make idl` | Generate IDL JSON from program source |
| `make cli ARGS="..."` | Run the IDL-driven CLI (auto-generated wrapper) |
| `make deploy` | Deploy program to sequencer |
| `make setup` | Mint a fresh public signer and save to `.chronicle_registry-state` |
| `make inspect` | Show ProgramId for the built binary |
| `make status` | Show saved state + binary info |
| `make clean` | Remove saved state |

## Prerequisites

- Rust + [risc0 toolchain](https://dev.risczero.com/api/zkvm/install)
- [logos-scaffold](https://github.com/logos-co/logos-scaffold) (`lgs`)
- A running LEZ sequencer
