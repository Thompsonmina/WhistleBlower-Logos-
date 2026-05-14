#!/usr/bin/env bash
#
# Minimal bootstrap for the whistleblower workspace.  Idempotent.
# Handles the bits that need to happen on every fresh clone or
# sequencer reset: ensure a sequencer is up, build the guest, build the
# host-side binaries, deploy the program, ensure the configured signer
# exists, open the registry.
#
#   1. Run `lgs setup` — initialises the local lgs config that
#      `localnet start`, `wallet`, and `deploy` all depend on.
#      Idempotent: re-runs are a no-op on a configured machine.
#   2. Ensure the LEZ sequencer is running on 127.0.0.1:3040 — start
#      it via `lgs localnet start` if nothing is listening.
#   3. Build the guest binary (risc0 docker build) if it isn't built yet.
#   4. Build the host-side binaries (chronicle_registry_cli + batch-anchor)
#      if they aren't built yet.  batch-anchor shells out to
#      chronicle_registry_cli, and step 6 below invokes batch-anchor
#      directly — so both must exist before we get there.
#   5. Deploy chronicle-registry (idempotent on-chain: program_id is the
#      binary hash, so re-deploying the same binary is a no-op).
#   6. Mint a Public signer if the one pinned in batch-anchor.toml
#      isn't in the wallet; rewrite both batch-anchor.{,it.}toml to
#      point at the new ID.
#   7. Call `batch-anchor init` to open the registry (idempotent).
#
# Assumes the LEZ sequencer is already running (port 3040) and docker
# is up if you also want nwaku (`batch-anchor node up` is on you).

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
PROD_TOML="$REPO_ROOT/batch-anchor/batch-anchor.toml"
IT_TOML="$REPO_ROOT/batch-anchor/batch-anchor.it.toml"
WALLET_HOME="$REPO_ROOT/.scaffold/wallet"
PROGRAM_BIN="$REPO_ROOT/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/chronicle_registry.bin"

cd "$REPO_ROOT"

CLI_BIN="$REPO_ROOT/target/debug/chronicle_registry_cli"
ANCHOR_BIN="$REPO_ROOT/batch-anchor/target/debug/batch-anchor"

# ── 1. Initialise lgs config ──────────────────────────────────────────
# Required on a fresh machine before localnet / wallet / deploy will
# work.  Idempotent on already-configured installs.
echo "→ lgs setup"
lgs setup

# ── 2. Ensure sequencer is up ─────────────────────────────────────────
# Probe port 3040 — start lgs localnet if nothing answers.  lgs's start
# command is idempotent (it noops if a sequencer is already running),
# but probing first lets us skip the spawn cost on hot runs.
if ! (exec 3<>/dev/tcp/127.0.0.1/3040) 2>/dev/null; then
    echo "→ Start LEZ sequencer (lgs localnet start)"
    lgs localnet start
else
    exec 3<&-
    echo "→ Sequencer already running on 127.0.0.1:3040"
fi

# ── 3. Build guest if missing ─────────────────────────────────────────
if [[ ! -f "$PROGRAM_BIN" ]]; then
    echo "→ Build guest (cold; ~3 min)"
    make build
else
    echo "→ Guest binary present, skipping build"
fi

# ── 4. Build host-side binaries if missing ────────────────────────────
if [[ ! -x "$CLI_BIN" ]]; then
    echo "→ Build chronicle_registry_cli"
    cargo build --bin chronicle_registry_cli
fi
if [[ ! -x "$ANCHOR_BIN" ]]; then
    echo "→ Build batch-anchor"
    (cd "$REPO_ROOT/batch-anchor" && cargo build --bin batch-anchor)
fi

# ── 5. Deploy (idempotent — same binary hash = same program_id) ───────
echo "→ Deploy chronicle-registry"
NSSA_WALLET_HOME_DIR="$WALLET_HOME" make deploy >/dev/null

# ── 6. Ensure the pinned signer exists in the wallet ──────────────────
# Both batch-anchor.toml and batch-anchor.it.toml pin the same signer
# (production and integration-test paths share the wallet + sequencer in
# our simple model — only the topic differs).  On a fresh clone the
# pinned ID isn't in the wallet, so mint a new Public and rewrite both
# tomls.  If they already disagree, prod is authoritative.
PINNED_SIGNER=$(grep -E '^signer_account_id' "$PROD_TOML" | sed -E 's/.*"([^"]+)".*/\1/')
echo "→ Verify signer $PINNED_SIGNER"
if NSSA_WALLET_HOME_DIR="$WALLET_HOME" lgs wallet -- account list 2>/dev/null \
     | grep -qF "Public/$PINNED_SIGNER"; then
    echo "  already in wallet"
    # Sync the IT toml to prod in case they drifted.
    sed -i.bak -E "s|^signer_account_id\s*=.*|signer_account_id = \"$PINNED_SIGNER\"|" "$IT_TOML"
    rm -f "$IT_TOML.bak"
else
    echo "  not in wallet — minting a new one"
    NEW_SIGNER=$(NSSA_WALLET_HOME_DIR="$WALLET_HOME" lgs wallet -- account new public 2>&1 \
        | sed -n 's|.*Public/\([A-Za-z0-9]\{32,\}\).*|\1|p' | head -1)
    [[ -n "$NEW_SIGNER" ]] || { echo "ERROR: could not mint signer" >&2; exit 1; }
    echo "  minted $NEW_SIGNER → rewriting both batch-anchor tomls"
    for toml in "$PROD_TOML" "$IT_TOML"; do
        sed -i.bak -E "s|^signer_account_id\s*=.*|signer_account_id = \"$NEW_SIGNER\"|" "$toml"
        rm -f "$toml.bak"
    done
fi

# ── 7. Open the registry (idempotent) ─────────────────────────────────
echo "→ Init registry"
(cd "$REPO_ROOT/batch-anchor" && "$ANCHOR_BIN" -c batch-anchor.it.toml init)

echo
echo "✅ IT setup complete."
echo "   run anchor:  cd batch-anchor && ./target/debug/batch-anchor -c batch-anchor.it.toml watch"
