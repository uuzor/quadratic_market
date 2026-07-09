# TxODDS Integration for QuadraticMarket

This package provides a TypeScript client for TxODDS's TxLINE API and utilities for converting StablePrice odds to LMSR q_values for seeding prediction markets.

## Overview

TxODDS provides consensus sports pricing (StablePrice) that blends odds from multiple bookmakers in real-time. This integration allows QuadraticMarket operators to seed markets with near-fair-value prices, reducing LP exposure to arbitrage from mispriced starting points.

## Key Concepts

### De-marginated vs Marginated Odds

TxODDS offers two types of odds:
- **Marginated**: Average market price including bookmaker overround (vig)
- **De-marginated**: Vig stripped out, representing true implied probability

**Use de-marginated odds for seeding** to avoid double-charging margin (bookmaker vig + your `buy_fee_bps`).

### LMSR Seeding

The LMSR (Logarithmic Market Scoring Rule) uses q_values to determine prices. With q_values:
- `q_values = [0, 0]` → 50/50 prices
- Higher q relative to others → higher price
- Lower q relative to others → lower price

Seeding from TxODDS odds means the market starts near the consensus fair value, so organic trading only needs to move the price for genuine information/opinion differences.

## Installation

```bash
npm install @quadratic-market/txodds
```

## Quick Start

### 1. Authenticate with TxODDS

```typescript
import { TxODDSClient } from "@quadratic-market/txodds";

const client = new TxODDSClient();

// Free tier (guest session - rate limited)
await client.startGuestSession();

// Paid tier (recommended for production)
await client.activateApiToken("your-api-token");
```

### 2. Fetch Fixtures and Odds

```typescript
// Get all upcoming fixtures
const fixtures = await client.getFixtures();

// Get odds for a specific fixture
const odds = await client.getOddsSnapshot(fixtureId);
```

### 3. Compute LMSR Q-Values

```typescript
import { 
  computeQValuesFromOdds, 
  qValuesToU64Array,
  validateQValues 
} from "@quadratic-market/txodds";

// LMSR B parameter (in raw lamports, e.g., 100_000_000 = 100 USDC)
const B = BigInt(100_000_000);

// Compute q_values from 1X2 (Home/Draw/Away) odds
const { qValues, oddsInfo } = computeQValuesFromOdds(
  odds, 
  "1X2",  // Market type
  B
);

// Validate the computed q_values
const validation = validateQValues(qValues, B);
if (!validation.valid) {
  console.error("Validation failed:", validation.errors);
}

// Convert to on-chain format
const onChainQValues = qValuesToU64Array(qValues);
console.log("Q-values:", onChainQValues);
```

### 4. Create Market on Solana

```typescript
import { QuadraticMarketClient } from "@quadratic-market/sdk";

// Create market with seeded q_values
await program.methods
  .createMarket({
    startTime: fixture.startTime,
    numOutcomes: 3,
    title: `${homeTeam} vs ${awayTeam}`,
    description: `Match winner for ${league}`,
    category: CATEGORY_SPORTS,
    lmsrBOverride: Some(B),
    initialQValues: Some(onChainQValues),
    marketMode: MarketMode.Lmsr,
  })
  .accounts({...})
  .rpc();
```

## API Reference

### TxODDSClient

```typescript
class TxODDSClient {
  // Authenticate
  startGuestSession(): Promise<string>
  activateApiToken(token: string): Promise<void>
  
  // Fetch data
  getFixtures(): Promise<Fixture[]>
  getOddsSnapshot(fixtureId: number): Promise<OddsEntry[]>
  getScoresSnapshot(fixtureId: number): Promise<ScoreEntry[]>
}
```

### Q-Value Calculator

```typescript
// Compute q_values from odds array
computeQValuesFromOdds(odds, marketType, b)

// Compute q_values from price array directly
computeQValuesFromPrices(targetPrices, b)

// Specialized 2-outcome calculation (closed form)
computeQValues2Outcomes(price0, b)

// Validate q_values produce reasonable prices
validateQValues(qValues, b)

// Convert to Solana-compatible u64 array
qValuesToU64Array(qValues)
```

## Best Practices

### 1. Use De-marginated Odds

Always use `oddsDemarginated` from TxODDS, not `oddsMarginated`. Using marginated odds plus your own `buy_fee_bps` creates double-margining.

### 2. Seed Only Once

Seed q_values **only at market creation**. The LMSR's mathematical properties (bounded loss) depend on q_values changing only through the buy/sell cost function tied to actual token mint/burn.

Do **not** try to re-seed q_values as odds move. If you want the market to track live odds, rely on organic trading—sharp bettors react to the same news feeds that TxODDS aggregates.

### 3. Size B Appropriately

`B` controls both the LP's worst-case bound (`b × ln(n)`) and the liquidity depth. For thin markets (e.g., International Friendlies):
- Use smaller B
- Be aware TxODDS itself is blending fewer bookmakers, so the seed is less reliable

### 4. Sanity Check Q-Values On-Chain

The contract already validates that seeded prices are between 1% and 99%. This prevents:
- API bugs returning absurd values
- Misconfigured bot settings
- Free arbitrage from clearly wrong prices

### 5. Handle Rate Limits

Guest sessions have strict rate limits. For production:
1. Use a paid API token
2. Implement caching of fixture/odds data
3. Batch market creation when possible

## Security Considerations

1. **External Data Source**: TxODDS odds come from an external API. Ensure:
   - Use HTTPS (enforced by the client)
   - Implement request signing if available
   - Cache data to reduce API dependency

2. **Bot Security**: The bot that fetches odds and creates markets:
   - Needs wallet private key
   - Should use a dedicated operator account, not admin
   - Implement proper key management (HSM, KMS, etc.)

3. **Error Handling**: The contract rejects markets with invalid q_values, but:
   - Monitor for failed validation on-chain
   - Log all API responses for debugging
   - Implement alerts for anomalous behavior

## Example Output

```
=== TxODDS Market Seeder Bot ===

Activating paid API token...
Authenticated successfully!

Fetching fixtures...
Found 150 fixtures
150 upcoming fixtures

Processing: Brazil vs Argentina
  Fixture ID: 17271370
  Start Time: 2024-07-10T20:00:00.000Z
  League: FIFA World Cup
  Found 3 odds entries
  Odds (de-marginated):
    Away: 33.3%
    Draw: 29.4%
    Home: 37.3%
  Q-values computed and validated:
    37.3% | 29.4% | 33.3%

  === CREATE MARKET PARAMETERS ===
  num_outcomes: 3
  start_time: 1720641600
  lmsr_b_override: 100000000
  initial_q_values: [1234567890, 0, 987654321]
  === END PARAMETERS ===
```

## License

MIT
