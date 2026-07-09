#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# run_lifecycle.sh — full two-phase protocol lifecycle on localnet
#   Phase 1: lifecycle/open_and_bet.ts       (open markets, place bets)
#   Phase 2: lifecycle/settle_and_withdraw.ts (settle, claim, withdraw)
#
# Starts one solana-test-validator, runs Phase 1, then Phase 2 (which waits
# out the on-chain timers itself), then cleans up.
# ─────────────────────────────────────────────────────────────

PROGRAM_ID="3MsEuMziRKjA1w1WTPeW5NvDUCjGoep2QZ5zBthGq23Z"
PROGRAM_SO="target/deploy/quadratic_market.so"
WALLET="${ANCHOR_WALLET:-$HOME/.config/solana/id.json}"
RPC_URL="http://127.0.0.1:8899"
LEDGER_DIR="/tmp/lifecycle-ledger"
VALIDATOR_LOG="/tmp/lifecycle-validator.log"

# Set up PATH with Solana and Anchor
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.cargo/bin:$HOME/.avm/bin:$PATH"
export ANCHOR_PROVIDER_URL="$RPC_URL"
export ANCHOR_WALLET="$WALLET"
export PROGRAM_ID
# Match the project's ts-mocha setup: transpile only, skip type checking.
export TS_NODE_TRANSPILE_ONLY=1

echo "=== Quadratic Market — Full Lifecycle Runner ==="
echo "Program ID : $PROGRAM_ID"
echo "Wallet     : $WALLET"
echo "RPC URL    : $RPC_URL"
echo ""
echo "Solana version: $(solana --version 2>/dev/null || echo 'not found')"
echo "Anchor version: $(anchor --version 2>/dev/null || echo 'not found')"
echo ""

echo ">>> Building program..."
cargo build-sbf 2>&1

echo ">>> Cleaning up old validator processes..."
pkill -f "solana-test-validator" 2>/dev/null || true
sleep 2
rm -rf "$LEDGER_DIR"
rm -f lifecycle/state.json

echo ">>> Starting solana-test-validator..."
setsid solana-test-validator \
  --bpf-program "$PROGRAM_ID" "$PROGRAM_SO" \
  --ledger "$LEDGER_DIR" \
  --reset \
  > "$VALIDATOR_LOG" 2>&1 &
VALIDATOR_PID=$!
echo "    Validator PID: $VALIDATOR_PID"
echo "    Log: $VALIDATOR_LOG"

echo ">>> Waiting for validator to become healthy (up to 60s)..."
ATTEMPTS=0
until curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); exit(0 if d.get('result')=='ok' else 1)" \
  2>/dev/null; do
  ATTEMPTS=$((ATTEMPTS + 1))
  if [ $ATTEMPTS -ge 60 ]; then
    echo "ERROR: Validator did not start"
    tail -20 "$VALIDATOR_LOG"
    kill $VALIDATOR_PID 2>/dev/null || true
    exit 1
  fi
  sleep 1
done
echo "    Validator healthy after ${ATTEMPTS}s"

echo ">>> Airdropping 100 SOL to wallet..."
solana airdrop 100 --keypair "$WALLET" --url "$RPC_URL" 2>/dev/null \
  && echo "    Airdrop done" || echo "    Airdrop skipped"

EXIT=0

echo ""
echo "─────────────────────────────────────────────────────"
echo ">>> PHASE 1 — open_and_bet.ts"
echo "─────────────────────────────────────────────────────"
npx ts-node --transpile-only -P lifecycle/tsconfig.json lifecycle/open_and_bet.ts || EXIT=$?

if [ $EXIT -eq 0 ]; then
  echo ""
  echo "─────────────────────────────────────────────────────"
  echo ">>> PHASE 2 — settle_and_withdraw.ts"
  echo "─────────────────────────────────────────────────────"
  npx ts-node --transpile-only -P lifecycle/tsconfig.json lifecycle/settle_and_withdraw.ts || EXIT=$?
fi

echo ""
echo ">>> Stopping validator (PID $VALIDATOR_PID)..."
kill $VALIDATOR_PID 2>/dev/null || pkill -f "solana-test-validator" 2>/dev/null || true
sleep 1
rm -rf "$LEDGER_DIR"

echo ""
if [ $EXIT -eq 0 ]; then
  echo "╔══════════════════════════════╗"
  echo "║   LIFECYCLE COMPLETE ✓       ║"
  echo "╚══════════════════════════════╝"
else
  echo "╔══════════════════════════════╗"
  echo "║   LIFECYCLE FAILED ✗         ║"
  echo "║   Exit code: $EXIT            ║"
  echo "╚══════════════════════════════╝"
fi
exit $EXIT
