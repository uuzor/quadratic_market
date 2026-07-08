# Quadratic Market Protocol

A Solana-based prediction market protocol with quadratic pricing (LMSR), P2P limit order trading, and epoch-based liquidity provision.

## Protocol Overview

### Core Features

1. **LMSR Pricing Engine**
   - Odds and prices are determined by the Logistic Market Scoring Rule (LMSR)
   - Initial seeding establishes the market liquidity
   - Dynamic pricing based on actual trading activity

2. **P2P Limit Order Trading**
   - Users cannot sell directly to the pool
   - All trades happen via a Polymarket-style P2P limit order book
   - Orders can be placed, filled, and cancelled

3. **Correlated Market Pricing**
   - Markets can be grouped for correlated pricing
   - Single-leg trades within groups receive correlation-adjusted pricing
   - Supports same-game state models and outcome masks

4. **Epoch-Based LP**
   - Deposit and withdraw operations are epoch-based
   - Prevents front-running and manipulation of liquidity
   - Epochs have configurable durations and cooling periods

## Deployment Status

### Devnet Deployment
- **Program ID**: `3MsEuMziRKjA1w1WTPeW5NvDUCjGoep2QZ5zBthGq23Z`
- **Network**: Solana Devnet
- **Status**: ✅ Fully Operational

### PDAs (Program Derived Addresses)

| Account | Address |
|---------|---------|
| GlobalConfig | `J56MNUwKXTSeiz859k1UaeDuYTTNwo9Aivyg1xNCLkbH` |
| LP Mint | `44KmsXyCA6bHS9wfeuqSsabQLSDF6s9Ck48iuEkW8FZt` |
| Base Mint | `G9Arp9inP2ocyYUT5NYR9LGie3ARSj3hdFidj3x5Admp` |
| Treasury | `2MA994gsUqkYQk83wSCm7QC5BzDiSrdgfniGd4rjcx1K` |
| Epoch 0 | `Faufh1Y7QTN4VidsY26RGt44n39ZnzUsKcooJ8eo59E6` |

### Admin
- **Address**: `AAYWKd4BqJ6VZ4e5oWnGR4kPWMQ9r3u4u4ibsyRNNovy`
- **Keypair**: `/tmp/devnet_wallet.json`

### Active Markets

| ID | Title | Outcomes | PDA |
|----|-------|----------|-----|
| 1 | Arsenal vs Liverpool - Match Result | 3 | `51YZPmBaBmDVCYQmMsarZ7L2hQV6aBaJwKYRZ1peuhKC` |
| 2 | Arsenal vs Liverpool - Both Teams To Score | 2 | `7Cr6zXyoxf5wZ4xRMYRRuxcVw5UJYFSd5CEGuFefaHqP` |

## Security Review Summary

### Critical Issues - All Fixed ✅

| Issue | Description | Status |
|-------|-------------|--------|
| EXPLOIT-01 | Cash-out double-dip | Fixed |
| EXPLOIT-02 | fill_order wrong-mint tokens | Fixed |
| EXPLOIT-03 | place_order escrow mint validation | Fixed |
| BUG-05 | OrderExpired missing | Fixed |

### Remaining Issues (Non-Critical)

| Issue | Severity | Description |
|-------|----------|-------------|
| Issue 1 | Low | `GlobalConfig::LEN` overcalculated by 9 bytes |

## Bot Setup

### Requirements
```bash
pip install -r bot/requirements.txt
```

### Configuration

Copy `.env.example` to `.env` and configure:

```bash
cd bot
cp .env.example .env
# Edit .env with your settings
```

### Key Environment Variables

| Variable | Description |
|----------|-------------|
| `RPC_URL` | Solana RPC endpoint |
| `PROGRAM_ID` | Deployed program ID |
| `BASE_MINT` | Trading token mint address |
| `OPERATOR_KEYPAIR_PATH` | Path to operator keypair |
| `ORACLE_KEYPAIR_PATH` | Path to oracle keypair |
| `ODDS_API_KEY` | The-Odds-API key for sports data |

### Running the API Server

```bash
cd bot
python api.py --port 8000
```

### API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /` | API information |
| `GET /health` | Health check |
| `GET /api/v1/config` | Global configuration |
| `GET /api/v1/markets` | List all markets |
| `GET /api/v1/markets/{id}` | Get market details |
| `GET /api/v1/markets/{id}/prices` | Get market prices |
| `GET /api/v1/epochs` | List epochs |
| `GET /api/v1/epochs/{id}` | Get epoch details |

## Project Structure

```
quadratic_market/
├── programs/
│   └── quadratic_market/    # Solana program (Anchor)
│       ├── src/
│       │   ├── instructions/ # Trading instructions
│       │   ├── state/       # Account state definitions
│       │   └── lib.rs       # Program entry
│       └── Cargo.toml
├── bot/                    # Python bot and API
│   ├── api.py              # FastAPI server
│   ├── bot.py              # Market creation bot
│   ├── chain.py            # On-chain interactions
│   ├── config.py           # Configuration
│   └── sports_api.py       # Sports data integration
├── frontend/               # Next.js frontend
├── tests/                  # Anchor tests
├── Anchor.toml            # Anchor configuration
└── README.md              # This file
```

## Build & Deploy

### Build Program
```bash
anchor build
```

### Deploy to Devnet
```bash
anchor deploy --provider.cluster devnet
```

### Initialize Protocol
```bash
cd bot
python init_protocol.py
```

### Create Markets
```bash
cd bot
python create_markets.py
```

## Testing

### Run Anchor Tests
```bash
anchor test
```

### Run Specific Test
```bash
anchor test --filter epoch
```

## License

MIT