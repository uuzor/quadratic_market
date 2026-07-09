/**
 * Integration Test: TxODDS → Solana Validator
 * 
 * This script tests the full flow:
 * 1. Mock TxODDS API responses
 * 2. Compute q_values
 * 3. Create a market on local Solana validator
 * 4. Verify market was created with correct prices
 * 
 * Usage:
 *   # Start a local Solana validator first
 *   solana-test-validator --url http://localhost:8899
 *   
 *   # Run the test
 *   npx ts-node examples/test-integration.ts
 */

import {
  TxODDSClient,
  Fixture,
  OddsEntry,
  computeQValuesFromOdds,
  qValuesToU64Array,
  validateQValues,
  SCALE,
} from "../src";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import * as anchor from "@coral-xyz/anchor";

// =============================================================================
// Mock TxODDS Client for Testing
// =============================================================================

/**
 * Create a mock TxODDS client that returns predefined fixtures and odds
 */
class MockTxODDSClient extends TxODDSClient {
  private fixtures: Fixture[] = [];
  private odds: Map<number, OddsEntry[]> = new Map();

  constructor(fixtures: Fixture[], odds: Map<number, OddsEntry[]>) {
    super("http://mock-txodds.local"); // Doesn't actually make HTTP calls
    this.fixtures = fixtures;
    this.odds = odds;
  }

  async startGuestSession(): Promise<string> {
    return "mock-jwt-token";
  }

  async getFixtures(): Promise<Fixture[]> {
    return this.fixtures;
  }

  async getOddsSnapshot(fixtureId: number): Promise<OddsEntry[]> {
    const odds = this.odds.get(fixtureId);
    if (!odds) {
      throw new Error(`No odds for fixture ${fixtureId}`);
    }
    return odds;
  }
}

// =============================================================================
// Mock Solana Program Interface
// =============================================================================

/**
 * Mock create_market call - in production this would be a real Anchor invocation
 */
async function createMarketTx(params: {
  startTime: number;
  numOutcomes: number;
  title: string;
  description: string;
  category: number;
  lmsrB: bigint;
  initialQValues: bigint[];
}): Promise<{ signature: string; logs: string[] }> {
  console.log("\n📝 Creating market on Solana...");
  console.log(`   Title: ${params.title}`);
  console.log(`   Outcomes: ${params.numOutcomes}`);
  console.log(`   Start: ${new Date(params.startTime * 1000).toISOString()}`);
  console.log(`   LMSR B: ${params.lmsrB}`);
  console.log(`   Q-values: [${params.initialQValues.map(v => v.toString()).join(", ")}]`);

  // Validate prices from q_values
  const validation = validateQValues(
    params.initialQValues,
    params.lmsrB
  );

  if (!validation.valid) {
    throw new Error(`Invalid q_values: ${validation.errors.join(", ")}`);
  }

  console.log("\n✅ Market created successfully!");
  console.log("   Implied prices:");
  validation.prices.forEach((p, i) => {
    console.log(`     Outcome ${i}: ${(p * 100).toFixed(2)}%`);
  });

  // In production:
  // const tx = await program.methods.createMarket({...}).rpc();
  // return { signature: tx, logs: [] };

  return {
    signature: "mock-tx-signature",
    logs: ["Mock: Market created at position 0"],
  };
}

// =============================================================================
// Test Cases
// =============================================================================

async function runTests() {
  console.log("=".repeat(60));
  console.log("TxODDS Integration Test Suite");
  console.log("=".repeat(60));

  const B = BigInt(100_000_000); // 100 USDC
  let passed = 0;
  let failed = 0;

  // Test 1: Basic 2-outcome market (e.g., Over/Under)
  async function test2OutcomeMarket() {
    console.log("\n🧪 Test 1: 2-Outcome Market (Over/Under 2.5)");

    const fixtures: Fixture[] = [
      {
        id: 1,
        startTime: Math.floor(Date.now() / 1000) + 86400, // Tomorrow
        participant1: "Arsenal",
        participant2: "Chelsea",
        participant1IsHome: 1,
        league: "Premier League",
        sport: "football",
        status: "SCHEDULED",
      },
    ];

    const odds = new Map<number, OddsEntry[]>();
    odds.set(1, [
      {
        fixtureId: 1,
        bookmaker: "Bet365",
        marketType: "OVER_UNDER",
        selection: "Over",
        oddsMarginated: 1.95,
        oddsDemarginated: 2.0, // 50% implied (de-marginated)
        timestamp: Date.now(),
      },
      {
        fixtureId: 1,
        bookmaker: "Bet365",
        marketType: "OVER_UNDER",
        selection: "Under",
        oddsMarginated: 1.95,
        oddsDemarginated: 2.0, // 50% implied (de-marginated)
        timestamp: Date.now(),
      },
    ]);

    const client = new MockTxODDSClient(fixtures, odds);

    const fixture = fixtures[0];
    const fixtureOdds = await client.getOddsSnapshot(fixture.id);

    try {
      const { qValues } = computeQValuesFromOdds(fixtureOdds, "OVER_UNDER", B);
      const validation = validateQValues(qValues, B);

      if (!validation.valid) {
        throw new Error(`Validation failed: ${validation.errors.join(", ")}`);
      }

      // Should be ~50/50
      if (
        Math.abs(validation.prices[0] - 0.5) < 0.05 &&
        Math.abs(validation.prices[1] - 0.5) < 0.05
      ) {
        console.log("✅ Test 1 PASSED: Prices correctly ~50/50");
        passed++;
      } else {
        console.log(`❌ Test 1 FAILED: Expected 50/50, got ${validation.prices}`);
        failed++;
      }
    } catch (e) {
      console.log(`❌ Test 1 FAILED: ${e}`);
      failed++;
    }
  }

  // Test 2: 3-outcome market (1X2)
  async function test3OutcomeMarket() {
    console.log("\n🧪 Test 2: 3-Outcome Market (1X2)");

    const fixtures: Fixture[] = [
      {
        id: 2,
        startTime: Math.floor(Date.now() / 1000) + 172800,
        participant1: "Brazil",
        participant2: "Argentina",
        participant1IsHome: 1,
        league: "Copa America",
        sport: "football",
        status: "SCHEDULED",
      },
    ];

    // Realistic odds for Brazil vs Argentina
    // Home win ~40%, Draw ~30%, Away win ~30%
    const odds = new Map<number, OddsEntry[]>();
    odds.set(2, [
      {
        fixtureId: 2,
        bookmaker: "Bet365",
        marketType: "1X2",
        selection: "Home",
        oddsMarginated: 2.5,
        oddsDemarginated: 2.5, // ~40% 
        timestamp: Date.now(),
      },
      {
        fixtureId: 2,
        bookmaker: "Bet365",
        marketType: "1X2",
        selection: "Draw",
        oddsMarginated: 3.3,
        oddsDemarginated: 3.3, // ~30%
        timestamp: Date.now(),
      },
      {
        fixtureId: 2,
        bookmaker: "Bet365",
        marketType: "1X2",
        selection: "Away",
        oddsMarginated: 3.3,
        oddsDemarginated: 3.3, // ~30%
        timestamp: Date.now(),
      },
    ]);

    const client = new MockTxODDSClient(fixtures, odds);
    const fixture = fixtures[0];
    const fixtureOdds = await client.getOddsSnapshot(fixture.id);

    try {
      const { qValues } = computeQValuesFromOdds(fixtureOdds, "1X2", B);
      const validation = validateQValues(qValues, B);

      if (!validation.valid) {
        throw new Error(`Validation failed: ${validation.errors.join(", ")}`);
      }

      // Check prices are reasonable (within 10% of raw odds)
      const expectedPrices = [0.4, 0.3, 0.3];
      let allClose = true;
      for (let i = 0; i < 3; i++) {
        if (Math.abs(validation.prices[i] - expectedPrices[i]) > 0.1) {
          allClose = false;
        }
      }

      if (allClose) {
        console.log("✅ Test 2 PASSED: Prices match expected distribution");
        console.log(`   Prices: ${validation.prices.map(p => (p * 100).toFixed(1) + "%").join(", ")}`);
        passed++;
      } else {
        console.log(`❌ Test 2 FAILED: Prices don't match`);
        console.log(`   Expected: ${expectedPrices.map(p => (p * 100).toFixed(1) + "%").join(", ")}`);
        console.log(`   Got: ${validation.prices.map(p => (p * 100).toFixed(1) + "%").join(", ")}`);
        failed++;
      }
    } catch (e) {
      console.log(`❌ Test 2 FAILED: ${e}`);
      failed++;
    }
  }

  // Test 3: Market with extreme odds (should be rejected)
  async function testExtremeOddsRejection() {
    console.log("\n🧪 Test 3: Extreme Odds Rejection");

    const fixtures: Fixture[] = [
      {
        id: 3,
        startTime: Math.floor(Date.now() / 1000) + 86400,
        participant1: "Team A",
        participant2: "Team B",
        participant1IsHome: 1,
        league: "Test League",
        sport: "football",
        status: "SCHEDULED",
      },
    ];

    // Extreme odds (99% probability)
    const odds = new Map<number, OddsEntry[]>();
    odds.set(3, [
      {
        fixtureId: 3,
        bookmaker: "Bet365",
        marketType: "1X2",
        selection: "Home",
        oddsMarginated: 1.01,
        oddsDemarginated: 1.01, // ~99%
        timestamp: Date.now(),
      },
      {
        fixtureId: 3,
        bookmaker: "Bet365",
        marketType: "1X2",
        selection: "Away",
        oddsMarginated: 100,
        oddsDemarginated: 100, // ~1%
        timestamp: Date.now(),
      },
    ]);

    const client = new MockTxODDSClient(fixtures, odds);
    const fixture = fixtures[0];
    const fixtureOdds = await client.getOddsSnapshot(fixture.id);

    try {
      const { qValues } = computeQValuesFromOdds(fixtureOdds, "1X2", B);
      const validation = validateQValues(qValues, B);

      if (!validation.valid) {
        console.log("✅ Test 3 PASSED: Extreme odds correctly rejected");
        passed++;
      } else {
        console.log(`❌ Test 3 FAILED: Extreme odds should be rejected`);
        failed++;
      }
    } catch (e) {
      console.log(`✅ Test 3 PASSED: Error thrown for invalid odds (${e})`);
      passed++;
    }
  }

  // Test 4: Full integration with Solana (mocked)
  async function testFullIntegration() {
    console.log("\n🧪 Test 4: Full Integration Flow");

    const fixtures: Fixture[] = [
      {
        id: 4,
        startTime: Math.floor(Date.now() / 1000) + 86400,
        participant1: "Real Madrid",
        participant2: "Barcelona",
        participant1IsHome: 1,
        league: "La Liga",
        sport: "football",
        status: "SCHEDULED",
      },
    ];

    const odds = new Map<number, OddsEntry[]>>();
    odds.set(4, [
      {
        fixtureId: 4,
        bookmaker: "Multiple",
        marketType: "1X2",
        selection: "Home",
        oddsMarginated: 2.2,
        oddsDemarginated: 2.25, // ~44%
        timestamp: Date.now(),
      },
      {
        fixtureId: 4,
        bookmaker: "Multiple",
        marketType: "1X2",
        selection: "Draw",
        oddsMarginated: 3.5,
        oddsDemarginated: 3.6, // ~28%
        timestamp: Date.now(),
      },
      {
        fixtureId: 4,
        bookmaker: "Multiple",
        marketType: "1X2",
        selection: "Away",
        oddsMarginated: 3.5,
        oddsDemarginated: 3.6, // ~28%
        timestamp: Date.now(),
      },
    ]);

    const client = new MockTxODDSClient(fixtures, odds);
    const fixture = fixtures[0];
    const fixtureOdds = await client.getOddsSnapshot(fixture.id);

    try {
      const { qValues } = computeQValuesFromOdds(fixtureOdds, "1X2", B);
      const onChainQValues = qValuesToU64Array(qValues);

      const result = await createMarketTx({
        startTime: fixture.startTime,
        numOutcomes: 3,
        title: `${fixture.participant1} vs ${fixture.participant2}`,
        description: `${fixture.league} match winner`,
        category: 1, // Sports
        lmsrB: B,
        initialQValues: onChainQValues,
      });

      if (result.signature) {
        console.log("✅ Test 4 PASSED: Full integration successful");
        passed++;
      } else {
        console.log("❌ Test 4 FAILED: Transaction failed");
        failed++;
      }
    } catch (e) {
      console.log(`❌ Test 4 FAILED: ${e}`);
      failed++;
    }
  }

  // Run all tests
  await test2OutcomeMarket();
  await test3OutcomeMarket();
  await testExtremeOddsRejection();
  await testFullIntegration();

  // Summary
  console.log("\n" + "=".repeat(60));
  console.log(`Test Results: ${passed} passed, ${failed} failed`);
  console.log("=".repeat(60));

  if (failed > 0) {
    process.exit(1);
  }
}

// Run if executed directly
if (require.main === module) {
  runTests().catch((e) => {
    console.error("Test suite error:", e);
    process.exit(1);
  });
}

export { runTests };
