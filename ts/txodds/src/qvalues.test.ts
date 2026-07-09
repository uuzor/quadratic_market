import {
  SCALE,
  priceToFp,
  fpToPrice,
  lmsrPriceFp,
  computeQValuesFromPrices,
  computeQValues2Outcomes,
  validateQValues,
} from "./qvalues";

describe("Q-Value Calculator", () => {
  const B = BigInt(100_000_000); // 100 USDC

  describe("SCALE constant", () => {
    it("should be 2^32", () => {
      expect(SCALE).toBe(BigInt(1) << 32n);
      expect(SCALE).toBe(4294967296n);
    });
  });

  describe("priceToFp / fpToPrice", () => {
    it("should convert decimal to fixed-point and back", () => {
      const testPrices = [0.1, 0.5, 0.75, 0.99];
      for (const price of testPrices) {
        const fp = priceToFp(price);
        const back = fpToPrice(fp);
        expect(Math.abs(back - price)).toBeLessThan(1e-6);
      }
    });

    it("should reject prices outside (0, 1)", () => {
      expect(() => priceToFp(0)).toThrow();
      expect(() => priceToFp(1)).toThrow();
      expect(() => priceToFp(-0.1)).toThrow();
      expect(() => priceToFp(1.1)).toThrow();
    });
  });

  describe("lmsrPriceFp", () => {
    it("should give equal prices for equal q_values", () => {
      const qValues = [0n, 0n, 0n];
      const p0 = lmsrPriceFp(qValues, B, 0);
      const p1 = lmsrPriceFp(qValues, B, 1);
      const p2 = lmsrPriceFp(qValues, B, 2);

      // Should be ~33.33% each
      expect(fpToPrice(p0)).toBeCloseTo(1 / 3, 2);
      expect(fpToPrice(p1)).toBeCloseTo(1 / 3, 2);
      expect(fpToPrice(p2)).toBeCloseTo(1 / 3, 2);
    });

    it("should give higher price to higher q_value", () => {
      const qValues = [BigInt(10_000_000), 0n]; // q0 > q1
      const p0 = lmsrPriceFp(qValues, B, 0);
      const p1 = lmsrPriceFp(qValues, B, 1);

      expect(fpToPrice(p0)).toBeGreaterThan(fpToPrice(p1));
    });

    it("should sum to ~1 across all outcomes", () => {
      const qValues = [
        BigInt(5_000_000),
        BigInt(3_000_000),
        BigInt(2_000_000),
      ];
      const sum = [0, 1, 2].reduce(
        (acc, i) => acc + fpToPrice(lmsrPriceFp(qValues, B, i)),
        0
      );
      expect(sum).toBeCloseTo(1, 2);
    });
  });

  describe("computeQValues2Outcomes", () => {
    it("should produce correct prices for 2 outcomes", () => {
      // 60/40 market
      const price0 = 0.6;
      const qValues = computeQValues2Outcomes(price0, B);

      const p0 = lmsrPriceFp(qValues, B, 0);
      const p1 = lmsrPriceFp(qValues, B, 1);

      expect(fpToPrice(p0)).toBeCloseTo(0.6, 2);
      expect(fpToPrice(p1)).toBeCloseTo(0.4, 2);
    });

    it("should handle 50/50 market", () => {
      const qValues = computeQValues2Outcomes(0.5, B);
      const p0 = lmsrPriceFp(qValues, B, 0);
      const p1 = lmsrPriceFp(qValues, B, 1);

      expect(fpToPrice(p0)).toBeCloseTo(0.5, 2);
      expect(fpToPrice(p1)).toBeCloseTo(0.5, 2);
    });
  });

  describe("computeQValuesFromPrices", () => {
    it("should converge to target prices for 3 outcomes", () => {
      // 50/30/20 market
      const targets = [0.5, 0.3, 0.2];
      const qValues = computeQValuesFromPrices(targets, B);

      const prices = [0, 1, 2].map((i) =>
        fpToPrice(lmsrPriceFp(qValues, B, i))
      );

      // Should be within 1% of target
      expect(prices[0]).toBeCloseTo(0.5, 1);
      expect(prices[1]).toBeCloseTo(0.3, 1);
      expect(prices[2]).toBeCloseTo(0.2, 1);
    });

    it("should reject prices that don't sum to ~1", () => {
      const invalidTargets = [0.5, 0.5]; // sums to 1.0 but should be for 2 outcomes
      expect(() => computeQValuesFromPrices(invalidTargets, B)).toThrow();
    });

    it("should handle even 50/50/50 market", () => {
      const targets = [1 / 3, 1 / 3, 1 / 3];
      const qValues = computeQValuesFromPrices(targets, B);

      const prices = [0, 1, 2].map((i) =>
        fpToPrice(lmsrPriceFp(qValues, B, i))
      );

      // All should be ~33%
      for (const p of prices) {
        expect(p).toBeCloseTo(1 / 3, 1);
      }
    });
  });

  describe("validateQValues", () => {
    it("should validate reasonable q_values", () => {
      const qValues = [BigInt(10_000_000), BigInt(5_000_000), BigInt(3_000_000)];
      const result = validateQValues(qValues, B);

      expect(result.valid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    it("should reject prices outside 1%-99%", () => {
      // Create q_values that produce extreme prices
      const qValues = [BigInt(1_000_000_000), 0n]; // Very high q0
      const result = validateQValues(qValues, B);

      expect(result.valid).toBe(false);
      expect(result.errors.length).toBeGreaterThan(0);
    });
  });
});
