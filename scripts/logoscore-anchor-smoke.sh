#!/usr/bin/env bash
# End-to-end smoke test for chronicle's on-chain anchor pipeline.
#
# Prerequisites:
#   1. chronicle-registry/ffi/target/release/libchronicle_registry_ffi.so
#      (cd chronicle-registry/ffi && cargo build --release)
#   2. chronicle-registry deployed on a running localnet; ANCHOR_PROGRAM_ID
#      must point at the on-chain program id.
#   3. A valid wallet at $ANCHOR_WALLET_HOME with the signer account known.
#
# Flow: load chronicle → setAnchorConfigJson → anchorBatchJson with one
# synthetic CID → assert tx_hash + listAnchorsJson contains a confirmed record.
#
# Each smoke run uses its own RUN_DIR / PERSIST_DIR so it doesn't see anchor
# records from a previous run.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOGOSCORE="${LOGOSCORE:-/nix/store/bsr21sfvfasl686mfi08r53g68pvc4gk-logos-logoscore-cli-bin-0.1.0/bin/logoscore}"
CHRONICLE_MODULES="${CHRONICLE_MODULES:-/tmp/chronicle-next-install/modules}"

# Anchor config — all overridable.
ANCHOR_PROGRAM_ID="${ANCHOR_PROGRAM_ID:-6ac5aa11b87bcd1961c7b5294b8d01e8746b2103e3beb665202a0299d2cf0252}"
ANCHOR_SEQUENCER_URL="${ANCHOR_SEQUENCER_URL:-http://127.0.0.1:3040}"
ANCHOR_WALLET_HOME="${ANCHOR_WALLET_HOME:-$(cd "${ROOT}/../.." && pwd)/chronicle-registry/.scaffold/wallet}"
ANCHOR_SIGNER_ACCOUNT="${ANCHOR_SIGNER_ACCOUNT:-CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r}"
FFI_LIB="${FFI_LIB:-$(cd "${ROOT}/../.." && pwd)/chronicle-registry/ffi/target/release/libchronicle_registry_ffi.so}"

RUN_DIR="${RUN_DIR:-/tmp/chronicle-anchor-smoke-$(date +%s)}"
LOG_DIR="$RUN_DIR/logoscore"
PERSIST_DIR="$RUN_DIR/persist"
DAEMON_LOG="$RUN_DIR/daemon.log"

mkdir -p "$LOG_DIR" "$PERSIST_DIR"

cleanup_run() {
  "$LOGOSCORE" --config-dir "$LOG_DIR" stop >/dev/null 2>&1 || true
  pkill -TERM -f "$RUN_DIR" 2>/dev/null || true
  sleep 0.5
  pkill -KILL -f "$RUN_DIR" 2>/dev/null || true
  local survivors
  survivors="$(pgrep -af "$RUN_DIR" | grep -v "pgrep -af" || true)"
  if [[ -n "$survivors" ]]; then
    echo "WARN: processes still tied to $RUN_DIR after cleanup:" >&2
    echo "$survivors" >&2
  fi
}
trap cleanup_run EXIT INT TERM

# ── Preflight ───────────────────────────────────────────────────────────────
[[ ! -f "$FFI_LIB" ]] && { echo "FFI .so not found at $FFI_LIB" >&2; echo "Run: cd chronicle-registry/ffi && cargo build --release" >&2; exit 1; }
[[ ! -d "$ANCHOR_WALLET_HOME" ]] && { echo "Wallet home not found at $ANCHOR_WALLET_HOME" >&2; exit 1; }
[[ ! -d "$CHRONICLE_MODULES" ]] && { echo "Chronicle modules not found at $CHRONICLE_MODULES" >&2; echo "Run: nix build path:$ROOT#install --out-link /tmp/chronicle-next-install" >&2; exit 1; }

# Sequencer reachability — bare TCP probe, the JSON-RPC endpoint doesn't
# answer GET / so curl-based checks return 405/etc.
if ! timeout 2 bash -c "</dev/tcp/$(echo "$ANCHOR_SEQUENCER_URL" | sed -E 's|^https?://||;s|/.*||;s|:|/|')" 2>/dev/null; then
  echo "Sequencer not reachable at $ANCHOR_SEQUENCER_URL" >&2
  echo "Run: cd chronicle-registry && lgs localnet start" >&2
  exit 1
fi

echo "smoke run: $RUN_DIR"
echo "  program_id   = $ANCHOR_PROGRAM_ID"
echo "  sequencer    = $ANCHOR_SEQUENCER_URL"
echo "  wallet_home  = $ANCHOR_WALLET_HOME"
echo "  signer       = $ANCHOR_SIGNER_ACCOUNT"
echo "  ffi_lib      = $FFI_LIB"

# ── Start logoscore with FFI path in env ────────────────────────────────────
CHRONICLE_REGISTRY_FFI_PATH="$FFI_LIB" "$LOGOSCORE" -D \
  --config-dir "$LOG_DIR" \
  --persistence-path "$PERSIST_DIR" \
  -m "$CHRONICLE_MODULES" \
  -v >"$DAEMON_LOG" 2>&1 &
disown

sleep 1

"$LOGOSCORE" --config-dir "$LOG_DIR" load-module chronicle >/dev/null

# ── Configure ───────────────────────────────────────────────────────────────
CONFIG_FILE="$RUN_DIR/anchor-config.json"
cat >"$CONFIG_FILE" <<EOF
{
  "program_id":        "$ANCHOR_PROGRAM_ID",
  "sequencer_url":     "$ANCHOR_SEQUENCER_URL",
  "wallet_home":       "$ANCHOR_WALLET_HOME",
  "signer_account_id": "$ANCHOR_SIGNER_ACCOUNT"
}
EOF
SET_RESULT="$("$LOGOSCORE" --config-dir "$LOG_DIR" call chronicle setAnchorConfigJson @"$CONFIG_FILE")"
RESULT="$SET_RESULT" python3 -c '
import json, os
w = json.loads(os.environ["RESULT"])
r = json.loads(w["result"])
assert r.get("ok") == True, f"setAnchorConfigJson failed: {r}"
assert r.get("configured") == True, f"not configured after set: {r}"
print("config saved")
'

# ── Anchor a synthetic CID ──────────────────────────────────────────────────
STAMP="$(date +%s)"
TEST_CID="zSmoke-${STAMP}-$(openssl rand -hex 4)"
TEST_HASH="$(openssl rand -hex 32)"
REQUEST_FILE="$RUN_DIR/anchor-request.json"
cat >"$REQUEST_FILE" <<EOF
{
  "entries": [
    { "cid": "$TEST_CID", "metadata_hash": "$TEST_HASH", "timestamp": $STAMP }
  ]
}
EOF

echo "anchoring cid=$TEST_CID timestamp=$STAMP"

ANCHOR_RESULT="$("$LOGOSCORE" --config-dir "$LOG_DIR" call chronicle anchorBatchJson @"$REQUEST_FILE")"
TX_HASH="$(RESULT="$ANCHOR_RESULT" python3 -c '
import json, os
w = json.loads(os.environ["RESULT"])
r = json.loads(w["result"])
assert r.get("ok") == True, f"anchorBatchJson failed: {r}"
tx = r.get("tx_hash")
assert tx, f"no tx_hash in response: {r}"
print(tx)
')"
echo "tx_hash=$TX_HASH"

# ── Verify local persistence ────────────────────────────────────────────────
LIST_RESULT="$("$LOGOSCORE" --config-dir "$LOG_DIR" call chronicle listAnchorsJson)"
RESULT="$LIST_RESULT" CID="$TEST_CID" python3 -c '
import json, os
w = json.loads(os.environ["RESULT"])
r = json.loads(w["result"])
assert r.get("ok") == True
anchors = r.get("anchors", {})
cid = os.environ["CID"]
assert cid in anchors, f"cid {cid} not in anchors map; got keys: {list(anchors.keys())}"
rec = anchors[cid]
assert rec["state"] == "confirmed", f"state not confirmed: {rec}"
assert rec["tx_hash"], f"missing tx_hash in persisted record: {rec}"
print("persisted: state={} tx_hash={}...".format(rec["state"], rec["tx_hash"][:16]))
'

# ── Verify the local cache is used by lookupAnchorJson ──────────────────────
LOOKUP_RESULT="$("$LOGOSCORE" --config-dir "$LOG_DIR" call chronicle lookupAnchorJson "$TEST_CID")"
RESULT="$LOOKUP_RESULT" python3 -c '
import json, os
w = json.loads(os.environ["RESULT"])
r = json.loads(w["result"])
assert r.get("ok") == True
assert r.get("found") == True, f"lookup missed cached entry: {r}"
print(f"lookup: found")
'

echo "ok cid=$TEST_CID tx_hash=$TX_HASH run_dir=$RUN_DIR"
