# Quadratic Market Txodds Bot

Automates the full market lifecycle using Txodds API:

1. **Create Markets** — fetches fixtures from Txodds, creates 3 markets per match:
   - 1X2 (3-way: Home, Draw, Away)
   - O/U 2.5 (2-way: Over, Under)
   - GG/NG (2-way: GG, NG)

2. **Update Odds** — fetches latest odds from Txodds, updates market odds until match start

3. **Suspend** — at `start_time`, suspends markets (no more bets)

4. **Settle with TxLINE Proof** — after `RESULT_DELAY_SECONDS`, fetches final score and settles markets using `settle_with_proof`

5. **Execute Slips** — backend executes bet slip legs (separate transaction per leg)

6. **Resolve Slips** — settles slip legs and resolves slips after all markets finalize

7. **Advance Epoch** — advances epoch when all markets in epoch are settled

## Setup

```bash
cd bot
cp .env.example .env
# Edit .env — fill in RPC_URL, keypair paths, PROGRAM_ID, BASE_MINT, TXODDS_API_KEY

pip install -r requirements.txt
```

Get a Txodds API key at https://txline.txodds.com (free tier available).

## Keypairs

The bot needs two keypairs:

| Keypair | Role | On-chain requirement |
|---|---|---|
| `OPERATOR_KEYPAIR_PATH` | Creates markets, suspends, finalizes, resolves slips | Must be added as operator via `add_operator` |
| `ORACLE_KEYPAIR_PATH` | Proposes results | Must match `global_config.oracle_pubkey` |

These can be the same keypair on devnet for testing.

## Running

```bash
# Continuous (recommended for production)
python bot.py

# Single pass (for cron)
python bot.py --once
```

## Market Types

Each match has 3 independent markets:

| Market | Category | Outcomes | Winning Outcome |
|---|---|---|---|
| 1X2 | 0 | Home, Draw, Away | Based on match result |
| O/U 2.5 | 1 | Over, Under | Based on total goals > 2.5 |
| GG/NG | 2 | GG, NG | Based on both teams scoring |

## Odds

Odds are provided by Txodds consensus pricing engine in basis points (BPS):
- 10000 BPS = 1.0x
- 20000 BPS = 2.0x
- 50000 BPS = 5.0x

## TxLINE Proof-Based Settlement (Key Feature!)

This demonstrates the **unique TxLINE primitives**:

```
Match Ends
    ↓
TxLINE API: Fetch Final Score (action=game_finalised)
    ↓
Bot calls settle_with_proof(...)
    ↓
On-Chain: Validates proof, settles market
    ↓
LP/User Balances Updated
```

**Verifiable**: Markets store `txline_fixture_id` and `txline_proof_verified = true` when settled this way.

## Correlation Matrix (LP Protection)

Market groups include a correlation matrix to protect LP from correlated parlay legs:

| Market Pair | Correlation | Discount |
|-------------|------------|----------|
| 1X2 ↔ O/U | 60% | 15% |
| 1X2 ↔ GG/NG | 70% | 17.5% |
| O/U ↔ GG/NG | 80% | 20% |

**Formula**: `payout = sum(leg_payouts) * (1 - correlation * 0.25)`

**Same-market rejection**: Cannot bet Home AND Away from same 1X2 market (mutually exclusive).

## State file

`bot_state.json` is written next to `bot.py`. It tracks:
- Market groups (matches)
- Individual markets (1X2, O/U, GG/NG)
- Slips for execution and settlement

Safe to delete to reset (markets already on-chain are unaffected).

## Supported Sports

Set `SPORTS` in `.env`:
```
SPORTS=soccer,football,basketball
```

## Architecture

```
bot/
├── bot.py           - Main loop with scheduled tasks
├── chain.py         - Solana chain interactions (markets, slips, epochs)
├── txodds_api.py    - Txodds API client (fixtures, odds, scores)
├── state.py         - Bot state management
├── config.py        - Environment configuration
└── requirements.txt
```
