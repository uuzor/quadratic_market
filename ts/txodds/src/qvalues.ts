/**
 * LMSR Q-Value Calculator
 * 
 * Converts de-marginated odds from TxODDS into LMSR q_values for
 * seeding prediction markets. This is the inverse of the LMSR price function.
 * 
 * LMSR Price Formula (Q32.32):
 *   price_i = exp(q_i / B) / sum(exp(q_j / B))
 * 
 * To invert for q_values given target prices:
 *   We solve for q_i such that the implied price matches the target.
 *   For a 2-outcome market: q_0 = B * ln(price_0 / (1 - price_0))
 * 
 * For N outcomes, we use an iterative approach since there's no closed-form
 * solution. We start from equal q_values and use gradient descent to find
 * q_values that produce the target prices.
 * 
 * @example
 * ```typescript
 * // Target prices from TxODDS de-marginated odds (as fractions, not decimals)
 * const targetPrices = [0.45, 0.30, 0.25]; // Home 45%, Draw 30%, Away 25%
 * const b = 100_000_000n; // B parameter in raw lamports
 * 
 * const qValues = computeQValuesFromPrices(targetPrices, b);
 * // Convert to u64 array for on-chain transaction
 * const qValuesArray = qValues.map(v => Number(v / SCALE_FP));
 * ```
 */

import { OddsEntry } from "./client";

// SCALE for Q32.32 fixed-point arithmetic (2^32)
export const SCALE = BigInt(1) << 32n;

// Helper to convert a decimal price (0-1) to Q32.32
export function priceToFp(price: number): bigint {
  if (price <= 0 || price >= 1) {
    throw new Error("Price must be between 0 and 1 (exclusive)");
  }
  return BigInt(Math.round(price * Number(SCALE)));
}

// Helper to convert Q32.32 back to decimal
export function fpToPrice(fp: bigint): number {
  return Number(fp) / Number(SCALE);
}

/**
 * LMSR price function in fixed-point arithmetic
 * Returns price for outcome i given q_values and B parameter
 */
export function lmsrPriceFp(
  qValues: bigint[],
  b: bigint,
  outcomeIdx: number
): bigint {
  if (qValues.length === 0 || b <= 0n) {
    throw new Error("Invalid parameters");
  }

  // Find max q for normalization
  const maxQ = qValues.reduce((max, q) => (q > max ? q : max), 0n);

  // Compute exp((q_i - max_q) / B) for each outcome
  const exps = qValues.map((q) => {
    if (q === maxQ) {
      return SCALE; // exp(0) = 1 in Q32.32
    }
    // exponent = (q - maxQ) / B, clamped to negative since q <= maxQ
    const diff = maxQ - q;
    const exponent = (diff * SCALE) / b;
    // Use approximation for exp(-x) where x is in Q32.32
    return expNegFp(exponent);
  });

  // Sum of exponentials
  const sumExp = exps.reduce((sum, e) => sum + e, 0n);

  // price_i = exp_i / sum_exp
  return (exps[outcomeIdx] * SCALE) / sumExp;
}

/**
 * Approximation for exp(-x) where x is in Q32.32
 * Uses Taylor series approximation for small values
 * For large x, returns ~0
 */
function expNegFp(x: bigint): bigint {
  if (x === 0n) {
    return SCALE;
  }

  const MAX_EXP = SCALE * 10n; // exp(-10) ≈ 0.000045
  if (x > MAX_EXP) {
    return 0n;
  }

  // For Q32.32, we need to compute exp(-x) where x is small
  // Use the approximation: exp(-x) ≈ 1 - x + x²/2 - x³/6 + ...
  // Working in Q32.32 arithmetic
  const xAbs = x;
  const xSq = (xAbs * xAbs) / SCALE;
  const xCu = (xSq * xAbs) / SCALE;
  const x4 = (xCu * xAbs) / SCALE;

  // 1 - x + x²/2 - x³/6 + x⁴/24
  let result = SCALE; // 1
  result -= xAbs; // -x
  result += xSq / 2n; // +x²/2
  result -= xCu / 6n; // -x³/6
  result += x4 / 24n; // +x⁴/24

  if (result < 0n) {
    return 0n;
  }
  return result;
}

/**
 * Compute q_values from target prices using Newton-Raphson optimization
 * 
 * @param targetPrices - Array of target probabilities (must sum to 1)
 * @param b - LMSR B parameter in raw lamports
 * @param maxIterations - Maximum optimization iterations (default 100)
 * @param tolerance - Convergence tolerance in Q32.32 (default 1 unit = ~2e-10)
 * @returns Array of q_values in Q32.32
 */
export function computeQValuesFromPrices(
  targetPrices: number[],
  b: bigint,
  maxIterations = 100,
  tolerance = 1n
): bigint[] {
  const n = targetPrices.length;
  if (n < 2) {
    throw new Error("Need at least 2 outcomes");
  }

  // Convert target prices to Q32.32
  const targets = targetPrices.map(priceToFp);

  // Verify prices sum to ~1
  const sumTarget = targets.reduce((sum, t) => sum + t, 0n);
  const toleranceSum = SCALE / 100n; // 1% tolerance
  if (
    sumTarget < SCALE - toleranceSum ||
    sumTarget > SCALE + toleranceSum
  ) {
    throw new Error(
      `Target prices must sum to ~1, got ${fpToPrice(sumTarget)}`
    );
  }

  // Initialize q_values to equal distribution (all zeros gives 1/n prices)
  let qValues = new Array<bigint>(n).fill(0n);

  // Use gradient descent to find q_values that produce target prices
  const learningRate = SCALE / 1000n; // 0.001 learning rate
  let prevError = BigInt(Number.MAX_SAFE_INTEGER);

  for (let iter = 0; iter < maxIterations; iter++) {
    let totalError = 0n;

    // Compute current prices and gradients
    const prices: bigint[] = [];
    for (let i = 0; i < n; i++) {
      prices.push(lmsrPriceFp(qValues, b, i));
    }

    // Update q_values using gradient descent
    // Gradient of error with respect to q_i: price_i - target_i
    for (let i = 0; i < n; i++) {
      const error = prices[i] - targets[i];
      totalError += error > 0n ? error : -error;

      // Update rule: q_i -= learning_rate * error
      const delta = (learningRate * error) / SCALE;
      qValues[i] -= delta;

      // Clamp q_values to reasonable range to prevent overflow
      const MAX_Q = b * 100n; // Reasonable upper bound
      const MIN_Q = -b * 100n; // Reasonable lower bound
      if (qValues[i] > MAX_Q) qValues[i] = MAX_Q;
      if (qValues[i] < MIN_Q) qValues[i] = MIN_Q;
    }

    // Check convergence
    if (totalError < tolerance) {
      break;
    }

    // Divergence check
    if (totalError > prevError * 3n) {
      // Reduce learning rate if diverging
      if (learningRate > SCALE / 10000n) {
        // Already at minimum rate
        break;
      }
    }
    prevError = totalError;
  }

  return qValues;
}

/**
 * Simplified 2-outcome q_value calculation
 * 
 * For exactly 2 outcomes, there's a closed-form solution:
 * q_0 = B * ln(price_0 / (1 - price_0))
 * q_1 = 0 (relative to q_0)
 */
export function computeQValues2Outcomes(
  price0: number,
  b: bigint
): bigint[] {
  if (price0 <= 0 || price0 >= 1) {
    throw new Error("Price must be between 0 and 1");
  }

  // q_0 = B * ln(price_0 / (1 - price_0))
  // In Q32.32, we compute: B * ln(ratio) where ratio = price0 / (1 - price0)
  const ratio = price0 / (1 - price0);
  const lnRatio = Math.log(ratio);

  // Convert ln(ratio) to Q32.32 and multiply by B
  const lnRatioFp = BigInt(Math.round(lnRatio * Number(SCALE)));
  const q0 = (b * lnRatioFp) / SCALE;

  return [q0, 0n];
}

/**
 * Simplified 3-outcome q_value calculation (1X2 market)
 * 
 * For 3 outcomes, we use a heuristic based on the relative probabilities.
 * This is less accurate than optimization but faster.
 */
export function computeQValues3OutcomesHeuristic(
  homeProb: number,
  drawProb: number,
  awayProb: number,
  b: bigint
): bigint[] {
  const total = homeProb + drawProb + awayProb;
  if (total === 0) {
    return [0n, 0n, 0n];
  }

  // Normalize
  const h = homeProb / total;
  const d = drawProb / total;
  const a = awayProb / total;

  // Use log-odds based calculation
  const qHome = b * BigInt(Math.round(Math.log(h / (1 - h)) * Number(SCALE) / Number(SCALE)));
  const qDraw = b * BigInt(Math.round(Math.log(d / (1 - d)) * Number(SCALE) / Number(SCALE)));
  const qAway = b * BigInt(Math.round(Math.log(a / (1 - a)) * Number(SCALE) / Number(SCALE)));

  return [qHome, qDraw, qAway];
}

/**
 * Extract de-marginated odds from TxODDS OddsEntry array for a specific market type
 * and compute q_values
 * 
 * @param oddsEntries - Array of odds entries from TxODDS
 * @param marketType - Market type to filter (e.g., "1X2" for 1X2 betting)
 * @param b - LMSR B parameter in raw lamports
 * @returns Object with computed q_values and odds info
 */
export function computeQValuesFromOdds(
  oddsEntries: OddsEntry[],
  marketType: string,
  b: bigint
): { qValues: bigint[]; oddsInfo: { selection: string; demarginated: number }[] } {
  // Filter to the specified market type
  const marketOdds = oddsEntries.filter(
    (o) => o.marketType === marketType
  );

  if (marketOdds.length === 0) {
    throw new Error(`No odds found for market type: ${marketType}`);
  }

  // Use de-marginated odds (true probabilities without bookmaker vig)
  // Sort by selection name for consistent ordering
  marketOdds.sort((a, b) => a.selection.localeCompare(b.selection));

  // Convert decimal odds to probabilities
  // De-marginated odds are already in probability form (0-1)
  const probabilities = marketOdds.map((o) => o.demarginatedOdds || 1 / o.oddsDemarginated);

  // Validate probabilities
  const sum = probabilities.reduce((s, p) => s + p, 0);
  if (sum <= 0 || sum > 2) {
    throw new Error(
      `Invalid probabilities from odds: sum = ${sum}. Expected values between 0-1.`
    );
  }

  // Normalize to sum to 1
  const normalizedProbs = probabilities.map((p) => p / sum);

  // Compute q_values based on number of outcomes
  let qValues: bigint[];
  if (normalizedProbs.length === 2) {
    qValues = computeQValues2Outcomes(normalizedProbs[0], b);
  } else {
    qValues = computeQValuesFromPrices(normalizedProbs, b);
  }

  return {
    qValues,
    oddsInfo: marketOdds.map((o) => ({
      selection: o.selection,
      demarginated: o.demarginatedOdds || 1 / o.oddsDemarginated,
    })),
  };
}

/**
 * Convert q_values to the format expected by the Solana program
 * (array of u64 values)
 */
export function qValuesToU64Array(qValues: bigint[]): bigint[] {
  // q_values can be negative in LMSR, but Solana expects u64
  // We shift all q_values so the minimum is 0, preserving relative differences
  const minQ = qValues.reduce((min, q) => (q < min ? q : min), 0n);
  
  if (minQ >= 0n) {
    // All non-negative, no shift needed
    return qValues;
  }

  // Shift so minimum is 0
  const shift = -minQ;
  return qValues.map((q) => q + shift);
}

/**
 * Validate that computed q_values produce reasonable prices
 * Returns the actual prices for verification
 */
export function validateQValues(
  qValues: bigint[],
  b: bigint
): { prices: number[]; valid: boolean; errors: string[] } {
  const errors: string[] = [];
  const prices: number[] = [];

  // Check each outcome price
  for (let i = 0; i < qValues.length; i++) {
    try {
      const priceFp = lmsrPriceFp(qValues, b, i);
      const price = fpToPrice(priceFp);
      prices.push(price);

      // Validate: price should be between 1% and 99%
      if (price < 0.01) {
        errors.push(`Outcome ${i}: price ${price.toFixed(4)} < 1% (too low)`);
      }
      if (price > 0.99) {
        errors.push(`Outcome ${i}: price ${price.toFixed(4)} > 99% (too high)`);
      }
    } catch (e) {
      errors.push(`Outcome ${i}: failed to compute price - ${e}`);
    }
  }

  // Check sum
  const sum = prices.reduce((s, p) => s + p, 0);
  if (Math.abs(sum - 1) > 0.01) {
    errors.push(`Prices sum to ${sum.toFixed(4)}, expected ~1.0`);
  }

  return {
    prices,
    valid: errors.length === 0,
    errors,
  };
}
