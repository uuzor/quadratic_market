#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# run_tests.sh — Run anchor tests without anchor CLI
# Replicates what `anchor test` does internally:
#   1. Start solana-test-validator with the program deployed
#   2. Run ts-mocha against localnet
#   3. Clean up
# ─────────────────────────────────────────────────────────────

# Program ID from Anchor.toml
PROGRAM_ID="4wKXu91KW6EBiecjUUYupQHjab6AULrGCm6hNrWbAvaA"
PROGRAM_SO="target/deploy/quadratic_market.so"

# Ensure the script uses the rustup-managed Cargo/Anchor and the Solana CLI.
export PATH="$HOME/.cargo/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"

# Default wallet path (can be overridden by ANCHOR_WALLET env var)
DEFAULT_WALLET="$HOME/.config/solana/id.json"
WALLET="${ANCHOR_WALLET:-$DEFAULT_WALLET}"

RPC_URL="${ANCHOR_PROVIDER_URL:-http://127.0.0.1:8899}"
LEDGER_DIR="/tmp/test-ledger-$$"
VALIDATOR_LOG="/tmp/validator-$$.log"

echo "=== Quadratic Market Test Runner ==="
echo "Program ID : $PROGRAM_ID"
echo "Wallet     : $WALLET"
echo "RPC URL    : $RPC_URL"
echo "Ledger     : $LEDGER_DIR"
echo ""

# ── Check prerequisites ──────────────────────────────────────

# Check if solana CLI is available
if ! command -v solana-test-validator &> /dev/null; then
    echo "ERROR: solana-test-validator not found in PATH"
    echo "Please install Solana CLI or add it to PATH"
    exit 1
fi

# Check if .so file exists
if [ ! -f "$PROGRAM_SO" ]; then
  echo ">>> Building program..."
  # Anchor must use the rustup-managed Cargo/Rust toolchain here; the
  # default Solana/Cargo shim on this machine is too old for edition 2024 deps.
  if [ -x "$HOME/.cargo/bin/anchor" ]; then
    CARGO="$HOME/.cargo/bin/cargo" RUSTC="$HOME/.cargo/bin/rustc" "$HOME/.cargo/bin/anchor" build 2>&1
  elif command -v cargo &> /dev/null; then
    cargo build-sbf 2>&1
  else
    echo "ERROR: Neither cargo nor anchor found. Cannot build program."
    exit 1
  fi
fi

if [ ! -f "$PROGRAM_SO" ]; then
  echo "ERROR: Program .so file still not found after build"
  exit 1
fi

# Check wallet exists
if [ ! -f "$WALLET" ]; then
  echo "WARNING: Wallet not found at $WALLET"
  echo "Creating temporary wallet for testing..."
  TEMP_WALLET="/tmp/test-wallet-$$.json"
  solana-keygen new --no-passphrase -o "$TEMP_WALLET" 2>/dev/null
  WALLET="$TEMP_WALLET"
fi

# ── 1. Kill any lingering test validator ───────────────────
echo ">>> Cleaning up old validator processes..."
pkill -f "solana-test-validator" 2>/dev/null || true
sleep 2
rm -rf "$LEDGER_DIR"

# ── 2. Start solana-test-validator ─────────────────────────
echo ">>> Starting solana-test-validator..."
setsid solana-test-validator \
  --bpf-program "$PROGRAM_ID" "$PROGRAM_SO" \
  --ledger "$LEDGER_DIR" \
  --reset \
  --quiet \
  > "$VALIDATOR_LOG" 2>&1 &
VALIDATOR_PID=$!
echo "    Validator PID: $VALIDATOR_PID"

# ── 3. Wait for validator to be ready ──────────────────────
echo ">>> Waiting for validator to become healthy (up to 60s)..."
ATTEMPTS=0
MAX_ATTEMPTS=60
until curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); exit(0 if d.get('result')=='ok' else 1)" \
  2>/dev/null; do
  ATTEMPTS=$((ATTEMPTS + 1))
  if [ $ATTEMPTS -ge $MAX_ATTEMPTS ]; then
    echo ""
    echo "ERROR: Validator did not start in ${MAX_ATTEMPTS}s"
    echo "Last 20 lines of validator log:"
    tail -20 "$VALIDATOR_LOG" 2>/dev/null
    kill $VALIDATOR_PID 2>/dev/null || true
    exit 1
  fi
  if [ $((ATTEMPTS % 10)) -eq 0 ]; then
    echo "    Still waiting... ${ATTEMPTS}s elapsed"
  fi
  sleep 1
done
echo "    Validator healthy after ${ATTEMPTS}s"

# ── 4. Fund the wallet ─────────────────────────────────────
echo ">>> Airdropping 100 SOL to test wallet..."
solana airdrop 100 \
  --keypair "$WALLET" \
  --url "$RPC_URL" \
  2>&1 | head -3 || echo "    Airdrop may have failed (wallet may already have funds)"

# ── 5. Run TypeScript tests ────────────────────────────────
echo ""
echo "─────────────────────────────────────────────────────"
echo ">>> Running TypeScript tests..."
echo "─────────────────────────────────────────────────────"

# Set env vars for anchor
export ANCHOR_PROVIDER_URL="$RPC_URL"
export ANCHOR_WALLET="$WALLET"

TEST_EXIT=0
npx ts-mocha \
  -p ./tsconfig.json \
  -t 1000000 \
  "tests/security_tests.ts" \
  "tests/order_book_test.ts" \
  "tests/lmsr_integration_test.ts" \
  2>&1 || TEST_EXIT=$?

# ── 6. Clean up ───────────────────────────────────────────
echo ""
echo ">>> Stopping validator (PID $VALIDATOR_PID)..."
kill $VALIDATOR_PID 2>/dev/null || pkill -f "solana-test-validator" 2>/dev/null || true
sleep 1
rm -rf "$LEDGER_DIR"
rm -f "$VALIDATOR_LOG"
rm -f "$TEMP_WALLET" 2>/dev/null

echo ""
if [ $TEST_EXIT -eq 0 ]; then
  echo "╔══════════════════════════════╗"
  echo "║    ALL TESTS PASSED ✓        ║"
  echo "╚══════════════════════════════╝"
else
  echo "╔══════════════════════════════╗"
  echo "║    SOME TESTS FAILED ✗       ║"
  echo "║    Exit code: $TEST_EXIT         ║"
  echo "╚══════════════════════════════╝"
fi

exit $TEST_EXIT
