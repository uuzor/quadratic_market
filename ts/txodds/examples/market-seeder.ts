/**
 * TxODDS Market Seeder Bot
 * 
 * Example bot that:
 * 1. Fetches fixtures from TxODDS
 * 2. Gets de-marginated odds for each fixture
 * 3. Computes LMSR q_values from odds
 * 4. Creates prediction markets on QuadraticMarket with seeded prices
 * 
 * Usage:
 *   npx ts-node examples/market-seeder.ts
 * 
 * Environment Variables:
 *   TXODDS_API_TOKEN - Optional. Paid API token for higher rate limits
 *   PRIVATE_KEY - Solana wallet private key (base58)
 *   RPC_URL - Solana RPC endpoint
 */

import {
  TxODDSClient,
  Fixture,
  computeQValuesFromOdds,
  qValuesToU64Array,
  validateQValues,
  SCALE,
} from "../src";

// Configuration
const CONFIG = {
  // LMSR B parameter (in raw lamports, e.g., 100_000_000 = 100 USDC)
  // Higher B = more liquidity, lower sensitivity to trades
  LMSR_B: BigInt(process.env.LMSR_B || "100_000_000"),
  
  // Minimum probability threshold (reject markets with odds outside this range)
  MIN_PROBABILITY: 0.01,  // 1%
  MAX_PROBABILITY: 0.99,  // 99%
  
  // Market type to use (1X2 = Home/Draw/Away)
  MARKET_TYPE: "1X2",
  
  // Categories for sports markets
  CATEGORY_SPORTS: 1,
};

// Selection mapping for 1X2 markets
const SELECTION_MAPPING: Record<string, number> = {
  "HOME": 0,
  "1": 0,
  "DRAW": 1,
  "X": 1,
  "AWAY": 2,
  "2": 2,
};

/**
 * Create a market seeder instance
 */
async function createMarketSeeder() {
  const client = new TxODDSClient();

  // Authenticate
  const apiToken = process.env.TXODDS_API_TOKEN;
  if (apiToken) {
    console.log("Activating paid API token...");
    await client.activateApiToken(apiToken);
  } else {
    console.log("Starting guest session (rate-limited)...");
    await client.startGuestSession();
  }

  console.log("Authenticated successfully!");
  return client;
}

/**
 * Process a fixture and create a seeded market
 */
async function processFixture(
  client: TxODDSClient,
  fixture: Fixture
): Promise<void> {
  console.log(`\nProcessing: ${fixture.participant1} vs ${fixture.participant2}`);
  console.log(`  Fixture ID: ${fixture.id}`);
  console.log(`  Start Time: ${new Date(fixture.startTime * 1000).toISOString()}`);
  console.log(`  League: ${fixture.league}`);

  // Skip if market already started
  const now = Math.floor(Date.now() / 1000);
  if (fixture.startTime <= now) {
    console.log("  Skipping: match already started");
    return;
  }

  try {
    // Fetch odds for this fixture
    const odds = await client.getOddsSnapshot(fixture.id);
    console.log(`  Found ${odds.length} odds entries`);

    // Compute q_values from 1X2 odds
    const { qValues, oddsInfo } = computeQValuesFromOdds(
      odds,
      CONFIG.MARKET_TYPE,
      CONFIG.LMSR_B
    );

    console.log("  Odds (de-marginated):");
    for (const info of oddsInfo) {
      console.log(`    ${info.selection}: ${(info.demarginated * 100).toFixed(1)}%`);
    }

    // Validate q_values
    const validation = validateQValues(qValues, CONFIG.LMSR_B);
    if (!validation.valid) {
      console.log("  Validation failed:");
      for (const error of validation.errors) {
        console.log(`    - ${error}`);
      }
      return;
    }

    console.log("  Q-values computed and validated:");
    console.log(`    ${validation.prices.map(p => (p * 100).toFixed(1) + "%").join(" | ")}`);

    // Convert to on-chain format
    const qValuesArray = qValuesToU64Array(qValues);
    
    // Log the values to use in create_market instruction
    console.log("\n  === CREATE MARKET PARAMETERS ===");
    console.log(`  num_outcomes: ${oddsInfo.length}`);
    console.log(`  start_time: ${fixture.startTime}`);
    console.log(`  lmsr_b_override: ${CONFIG.LMSR_B.toString()}`);
    console.log(`  initial_q_values: [${qValuesArray.map(v => v.toString()).join(", ")}]`);
    console.log("  === END PARAMETERS ===\n");

    // In production, you would call the Solana program here:
    // await createMarketTransaction(
    //   fixture.id,
    //   oddsInfo.length,
    //   fixture.startTime,
    //   `${fixture.participant1} vs ${fixture.participant2}`,
    //   fixture.league,
    //   CONFIG.LMSR_B,
    //   qValuesArray,
    //   wallet
    // );

  } catch (error) {
    console.error(`  Error processing fixture: ${error}`);
  }
}

/**
 * Main function to sync fixtures and create markets
 */
async function main() {
  console.log("=== TxODDS Market Seeder Bot ===\n");

  try {
    const client = await createMarketSeeder();

    // Fetch all fixtures
    console.log("\nFetching fixtures...");
    const fixtures = await client.getFixtures();
    console.log(`Found ${fixtures.length} fixtures`);

    // Filter to relevant fixtures (not started, with odds)
    const upcomingFixtures = fixtures.filter((f) => {
      const now = Math.floor(Date.now() / 1000);
      return f.startTime > now && f.status === "SCHEDULED";
    });

    console.log(`${upcomingFixtures.length} upcoming fixtures`);

    // Process each fixture
    // In production, you'd batch these or run them in parallel with rate limiting
    for (const fixture of upcomingFixtures.slice(0, 5)) {
      await processFixture(client, fixture);
    }

    console.log("\n=== Done ===");

  } catch (error) {
    console.error("Fatal error:", error);
    process.exit(1);
  }
}

// Run if executed directly
if (require.main === module) {
  main();
}

export { main, createMarketSeeder, processFixture };
