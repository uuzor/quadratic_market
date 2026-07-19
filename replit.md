# Quadratic Market — Solana Prediction Market Protocol

## Overview
Decentralized sports betting and prediction market platform built on the Solana blockchain. Uses an Automated Market Maker (AMM) based on the Logarithmic Market Scoring Rule (LMSR) for pricing outcomes.

## Tech Stack
- **Blockchain**: Solana (program ID: `9H1DCo5QaUtiMne4UH44aefHyv8Xpc8EgZwrshRZqsLC`)
- **Smart Contracts**: Rust + Anchor Framework v0.32.1
- **Tests**: TypeScript + ts-mocha + @coral-xyz/anchor
- **Oracle Bot**: Python (bot/)

## Project Structure
```
programs/quadratic_market/   Rust on-chain program
  src/lib.rs                 Program entry point (all instructions)
  src/state/                 Account data structures
  src/math/                  LMSR pricing math
  src/epoch_ops.rs           Epoch lifecycle management
  src/liquidity.rs           LP operations
  src/trade.rs               Buy/sell shares
  src/settlement.rs          Oracle result + finalization
tests/
  epoch_trade_simulation_test.ts  Full epoch lifecycle sim (2 markets)
  epoch_user_flow_test.ts    Single epoch flow test
  quadratic_market.ts        Happy-path tests
  protocol_tests.ts          Security & edge case tests
  simulation_test.ts         High-volume simulation
bot/                         Python oracle automation bot
```

## Running Tests

### Prerequisites (already set up)
- Solana CLI: `~/.local/share/solana/install/active_release/bin/`
- Wallet: `/home/runner/.config/solana/id.json`
- Built program: `target/deploy/quadratic_market.so`

### Run All Tests
```bash
./run_tests.sh
```

### Run Specific Test File
Set `ANCHOR_PROVIDER_URL` and `ANCHOR_WALLET`, start `solana-test-validator`, then:
```bash
npx ts-mocha -p ./tsconfig.json -t 1000000 "tests/epoch_trade_simulation_test.ts"
```

### Build Program
```bash
export CARGO_HOME="/home/runner/workspace/.local/share/.cargo"
export RUSTUP_HOME="/home/runner/.rustup"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$CARGO_HOME/bin:$PATH"
cargo build-sbf
```

## Key Epoch Lifecycle
1. `init_epoch` — open new epoch (operator)
2. `add_liquidity` — LP funds pool
3. `create_market` — create binary prediction markets
4. `buy_shares` / `sell_shares` — traders place bets
5. `suspend_market` → `settle_with_proof` — settle markets with TxLINE proof
6. `request_withdraw` → `process_withdrawal` — LP exits

## User Preferences
- Tests use `challenge_window_seconds = 0` to avoid long waits
- Program compiled with `cargo build-sbf` (not `anchor build`)
- Wallet stored at `/home/runner/.config/solana/id.json`
