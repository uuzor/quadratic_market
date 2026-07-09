/**
 * @quadratic-market/txodds
 * 
 * TxODDS API client and LMSR q_value calculator for QuadraticMarket
 */

// Client exports
export {
  TxODDSClient,
  Fixture,
  OddsEntry,
  ScoreEntry,
} from "./client";

// Q-value calculator exports
export {
  SCALE,
  priceToFp,
  fpToPrice,
  computeQValuesFromPrices,
  computeQValues2Outcomes,
  computeQValues3OutcomesHeuristic,
  computeQValuesFromOdds,
  qValuesToU64Array,
  validateQValues,
} from "./qvalues";
