use crate::constants::{
    CORRELATION_MAX_BPS, MAX_OUTCOMES, MAX_SAME_GAME_STATES, SCALE,
};
use crate::errors::QuadraticMarketError;
use crate::state::CorrelationPair;
use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default, Copy, PartialEq, Eq)]
pub struct LogicalOutcome {
    pub market_index: u8,
    pub outcome_id: u8,
    /// Bitset of event states where this outcome is true.
    ///
    /// Example for 1X in a 1/X/2 state model:
    /// state 0 = home win, state 1 = draw, state 2 = away win
    /// 1X mask = 0b011.
    pub state_mask: u64,
}

pub fn compute_joint_probability_fp(
    outcomes: &[LogicalOutcome],
    state_probabilities: &[u64],
    num_states: u8,
) -> Result<u64> {
    require!(num_states > 0, QuadraticMarketError::InvalidAmount);
    require!(
        num_states as usize <= MAX_SAME_GAME_STATES,
        QuadraticMarketError::InvalidAmount
    );
    require!(
        state_probabilities.len() >= num_states as usize,
        QuadraticMarketError::InvalidAmount
    );
    require!(!outcomes.is_empty(), QuadraticMarketError::InvalidAmount);

    let mut joint_mask = if num_states as usize == MAX_SAME_GAME_STATES {
        u64::MAX
    } else {
        (1u64 << num_states) - 1
    };

    for outcome in outcomes {
        joint_mask &= outcome.state_mask;
    }

    require!(joint_mask != 0, QuadraticMarketError::InvalidAmount);

    let mut probability: u128 = 0;
    for state_idx in 0..num_states as usize {
        if (joint_mask & (1u64 << state_idx)) != 0 {
            probability = probability
                .checked_add(state_probabilities[state_idx] as u128)
                .ok_or(QuadraticMarketError::MathOverflow)?;
        }
    }

    require!(probability > 0, QuadraticMarketError::InvalidAmount);
    require!(
        probability <= SCALE as u128,
        QuadraticMarketError::InvalidAmount
    );

    Ok(probability as u64)
}

pub fn compute_same_game_combined_odds_fp(
    outcomes: &[LogicalOutcome],
    state_probabilities: &[u64],
    num_states: u8,
    house_margin_bps: u64,
    bonus_multiplier_bps: u64,
    statistical_discount_bps: u64,
) -> Result<u64> {
    require!(
        statistical_discount_bps <= CORRELATION_MAX_BPS,
        QuadraticMarketError::CorrelationOutOfBounds
    );

    let joint_probability =
        compute_joint_probability_fp(outcomes, state_probabilities, num_states)?;

    let margin_factor = CORRELATION_MAX_BPS
        .checked_sub(house_margin_bps)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    let mut odds = ((SCALE as u128) << 32)
        .checked_div(joint_probability as u128)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    odds = odds
        .checked_mul(margin_factor as u128)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / CORRELATION_MAX_BPS as u128;

    if statistical_discount_bps != CORRELATION_MAX_BPS {
        odds = odds
            .checked_mul(statistical_discount_bps as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / CORRELATION_MAX_BPS as u128;
    }

    if bonus_multiplier_bps != CORRELATION_MAX_BPS {
        odds = odds
            .checked_mul(bonus_multiplier_bps as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / CORRELATION_MAX_BPS as u128;
    }

    Ok(odds as u64)
}

/// Compute adjusted q_values for a market given its directional correlations with other markets in the group.
///
/// For each correlation pair (market_a, outcome_a) → (market_b, outcome_b, weight):
///   If we're computing adjusted q for market_b at `this_market_index`, outcome `i`:
///     when pair.market_b_index == this_market_index && pair.outcome_b_id == i:
///       adjusted[i] += (pair.weight_bps * q_a[pair.outcome_a_id]) / CORRELATION_MAX_BPS
///
/// This preserves the directional signal: heavy flow on BTTS Yes only affects
/// the specific correlated outcomes (e.g., Over 2.5), not all outcomes equally.
pub fn compute_adjusted_q_values(
    market_q_values: &[u64; MAX_OUTCOMES],
    num_outcomes: u8,
    this_market_index: u8,
    correlated_market_q_values: &[[u64; MAX_OUTCOMES]; MAX_OUTCOMES],
    correlations: &[CorrelationPair],
    num_correlations: u8,
) -> Result<[u64; MAX_OUTCOMES]> {
    let mut adjusted = *market_q_values;

    let mut pair_idx: u8 = 0;
    while pair_idx < num_correlations {
        let pair = &correlations[pair_idx as usize];

        // Only apply correlations where this market is the target (market_b)
        if pair.market_b_index == this_market_index {
            // Get the source market's q_value for the specific correlated outcome
            let source_market_q = &correlated_market_q_values[pair.market_a_index as usize];
            if (pair.outcome_a_id as usize) < MAX_OUTCOMES {
                let q_source = source_market_q[pair.outcome_a_id as usize];

                if q_source > 0 && pair.weight_bps > 0 {
                    // adjustment = (weight_bps * q_source) / CORRELATION_MAX_BPS
                    let adjustment = (pair.weight_bps as u128)
                        .checked_mul(q_source as u128)
                        .ok_or(QuadraticMarketError::CorrelationOverflow)?
                        / CORRELATION_MAX_BPS as u128;

                    // Add to ALL outcomes of the target market proportionally
                    // The correlated outcome gets the full adjustment, other outcomes get 0
                    if (pair.outcome_b_id as usize) < num_outcomes as usize {
                        adjusted[pair.outcome_b_id as usize] = adjusted[pair.outcome_b_id as usize]
                            .checked_add(adjustment as u64)
                            .ok_or(QuadraticMarketError::CorrelationOverflow)?;
                    }
                }
            }
        }

        pair_idx = pair_idx
            .checked_add(1)
            .ok_or(QuadraticMarketError::CorrelationOverflow)?;
    }

    Ok(adjusted)
}

/// Compute the multiplicative combined odds from multiple legs.
/// Each leg's probability is computed as its LMSR price.
/// combined_probability = product(p_i) / SCALE^(n-1)
/// Returns combined odds in basis points.
pub fn compute_combined_odds_bps(leg_probabilities: &[u64], num_legs: u8) -> Result<u64> {
    if num_legs == 0 {
        return Err(QuadraticMarketError::InvalidAmount.into());
    }

    if num_legs == 1 {
        // Single leg: odds = SCALE / probability (in Q32.32)
        let p = leg_probabilities[0];
        require!(p > 0, QuadraticMarketError::InvalidAmount);
        let odds_fp = ((crate::constants::SCALE as u128) << 32)
            .checked_div(p as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        // Convert Q32.32 to bps: (odds_fp / SCALE) * 10000
        let odds_bps = (odds_fp / crate::constants::SCALE as u128)
            .checked_mul(10_000)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        return Ok(odds_bps as u64);
    }

    // Multi-leg: combined_probability = product(p_i) / SCALE^(n-1)
    let mut combined_prob: u128 = crate::constants::SCALE as u128;

    let mut i: usize = 0;
    while i < num_legs as usize {
        let p = leg_probabilities[i];
        require!(p > 0, QuadraticMarketError::InvalidAmount);
        combined_prob = combined_prob
            .checked_mul(p as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / crate::constants::SCALE as u128;
        i += 1;
    }

    require!(combined_prob > 0, QuadraticMarketError::InvalidAmount);

    // combined_odds = SCALE / combined_prob
    let combined_odds_fp = ((crate::constants::SCALE as u128) << 32)
        .checked_div(combined_prob)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Convert to bps
    let odds_bps = (combined_odds_fp / crate::constants::SCALE as u128)
        .checked_mul(10_000)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    Ok(odds_bps as u64)
}


/// Compute the multiplicative combined odds from multiple legs, with house margin and bonus.
/// Each leg price is an LMSR price (Q32.32 probability).
///
/// For each leg:
///   decimal_odds = SCALE^2 / price     (Q32.32)
///   odds_with_margin = odds * (CORRELATION_MAX_BPS - house_margin_bps) / CORRELATION_MAX_BPS
///
/// Combined = product of all margin-adjusted odds (dividing by SCALE between multiplications)
/// Then apply bonus_multiplier if applicable.
///
/// Returns combined odds in Q32.32 fixed-point (decimal odds, e.g., 2.5x, 32x).
pub fn compute_combined_odds_fp(
    leg_probabilities: &[u64],
    num_legs: u8,
    house_margin_bps: u64,
    bonus_multiplier_bps: u64,
) -> Result<u64> {
    if num_legs == 0 {
        return Err(QuadraticMarketError::InvalidAmount.into());
    }

    let margin_factor = CORRELATION_MAX_BPS
        .checked_sub(house_margin_bps)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Start with SCALE (1.0 in Q32.32) for the running combined odds
    let mut combined_odds: u128 = SCALE as u128;

    let mut i: usize = 0;
    while i < num_legs as usize {
        let p = leg_probabilities[i];
        require!(p > 0, QuadraticMarketError::InvalidAmount);

        // decimal_odds = SCALE^2 / p (Q32.32)
        let odds_fp = ((SCALE as u128) << 32)
            .checked_div(p as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?;

        // Apply house margin: odds * margin_factor / CORRELATION_MAX_BPS
        let odds_with_margin = odds_fp
            .checked_mul(margin_factor as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / CORRELATION_MAX_BPS as u128;

        // Multiply into combined: combined * odds_with_margin / SCALE
        combined_odds = combined_odds
            .checked_mul(odds_with_margin)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / SCALE as u128;

        i += 1;
    }

    require!(combined_odds > 0, QuadraticMarketError::InvalidAmount);

    // Apply bonus multiplier
    if bonus_multiplier_bps != CORRELATION_MAX_BPS {
        combined_odds = combined_odds
            .checked_mul(bonus_multiplier_bps as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / CORRELATION_MAX_BPS as u128;
    }

    Ok(combined_odds as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_correlation_passthrough() {
        let q = [10_000_000u64, 5_000_000u64, 0, 0, 0, 0, 0, 0];
        let correlated = [[0u64; MAX_OUTCOMES]; MAX_OUTCOMES];
        let correlations: [CorrelationPair; 16] = [CorrelationPair::default(); 16];

        let adjusted = compute_adjusted_q_values(&q, 2, 0, &correlated, &correlations, 0).unwrap();

        assert_eq!(adjusted, q);
    }

    #[test]
    fn test_single_directional_pair() {
        // Market 0 (BTTS): q = [10M, 5M] (Yes, No)
        // Market 1 (Over/Under): q = [3M, 2M] (Over, Under)
        // Correlation: BTTS Yes (0,0) → Over 2.5 (1,0) at 7500 bps
        let q_market1 = [3_000_000u64, 2_000_000u64, 0, 0, 0, 0, 0, 0];
        let q_market0 = [10_000_000u64, 5_000_000u64, 0, 0, 0, 0, 0, 0];

        let mut correlated = [[0u64; MAX_OUTCOMES]; MAX_OUTCOMES];
        correlated[0] = q_market0;
        correlated[1] = q_market1;

        let mut correlations: [CorrelationPair; 16] = [CorrelationPair::default(); 16];
        correlations[0] = CorrelationPair {
            market_a_index: 0,
            outcome_a_id: 0, // BTTS Yes
            market_b_index: 1,
            outcome_b_id: 0, // Over 2.5
            weight_bps: 7500,
        };

        // Compute adjusted q for market 1 (Over/Under)
        let adjusted =
            compute_adjusted_q_values(&q_market1, 2, 1, &correlated, &correlations, 1).unwrap();

        // Only Over 2.5 (outcome 0) should be adjusted
        let expected_adjustment = (7500 * 10_000_000) / 10_000; // 7_500_000
        assert_eq!(adjusted[0], q_market1[0] + expected_adjustment);
        // Under 2.5 (outcome 1) should be unchanged
        assert_eq!(adjusted[1], q_market1[1]);
    }

    #[test]
    fn test_multiple_pairs_cumulative() {
        // Two correlations affecting the same target outcome
        let q_target = [1_000_000u64, 500_000u64, 0, 0, 0, 0, 0, 0];
        let q_source_a = [5_000_000u64, 0, 0, 0, 0, 0, 0, 0];
        let q_source_b = [3_000_000u64, 0, 0, 0, 0, 0, 0, 0];

        let mut correlated = [[0u64; MAX_OUTCOMES]; MAX_OUTCOMES];
        correlated[0] = q_source_a;
        correlated[1] = q_source_b;
        correlated[2] = q_target;

        let mut correlations: [CorrelationPair; 16] = [CorrelationPair::default(); 16];
        correlations[0] = CorrelationPair {
            market_a_index: 0,
            outcome_a_id: 0,
            market_b_index: 2,
            outcome_b_id: 0,
            weight_bps: 5000,
        };
        correlations[1] = CorrelationPair {
            market_a_index: 1,
            outcome_a_id: 0,
            market_b_index: 2,
            outcome_b_id: 0,
            weight_bps: 3000,
        };

        let adjusted =
            compute_adjusted_q_values(&q_target, 2, 2, &correlated, &correlations, 2).unwrap();

        let adj_a = (5000 * 5_000_000) / 10_000; // 2_500_000
        let adj_b = (3000 * 3_000_000) / 10_000; // 900_000
        assert_eq!(adjusted[0], q_target[0] + adj_a + adj_b);
    }

    #[test]
    fn test_zero_weight_no_adjustment() {
        let q = [10_000_000u64, 5_000_000u64, 0, 0, 0, 0, 0, 0];
        let correlated = [[0u64; MAX_OUTCOMES]; MAX_OUTCOMES];
        let mut correlations: [CorrelationPair; 16] = [CorrelationPair::default(); 16];
        correlations[0] = CorrelationPair {
            market_a_index: 0,
            outcome_a_id: 0,
            market_b_index: 0,
            outcome_b_id: 0,
            weight_bps: 0, // zero weight
        };

        let adjusted = compute_adjusted_q_values(&q, 2, 0, &correlated, &correlations, 1).unwrap();

        assert_eq!(adjusted, q);
    }

    #[test]
    fn test_combined_odds_single_leg() {
        // Price of 0.5 (50% probability) in Q32.32
        let p = crate::constants::SCALE / 2;
        let odds = compute_combined_odds_bps(&[p], 1).unwrap();
        // Odds should be 2.0 = 20000 bps
        assert!(
            odds >= 19900 && odds <= 20100,
            "Expected ~20000 bps, got {}",
            odds
        );
    }

    #[test]
    fn test_combined_odds_two_independent_legs() {
        // Two legs each at 50% probability
        let p = crate::constants::SCALE / 2;
        let odds = compute_combined_odds_bps(&[p, p], 2).unwrap();
        // Combined: 0.5 * 0.5 = 0.25 → odds = 4.0 = 40000 bps
        assert!(
            odds >= 39800 && odds <= 40200,
            "Expected ~40000 bps, got {}",
            odds
        );
    }


    #[test]
    fn test_combined_odds_fp_no_margin() {
        // Single leg at 50% probability, no margin, no bonus
        let p = SCALE / 2;
        let odds = compute_combined_odds_fp(&[p], 1, 0, CORRELATION_MAX_BPS).unwrap();
        // Expected: 2.0 in Q32.32 = 2 * SCALE = 8589934592
        let expected = 2 * SCALE;
        assert!(
            (odds as i64 - expected as i64).unsigned_abs() < SCALE / 100,
            "Expected ~2.0 ({}), got {}",
            expected,
            odds
        );
    }

    #[test]
    fn test_combined_odds_fp_with_margin() {
        // Single leg at 50% probability, 5% margin
        let p = SCALE / 2;
        let odds = compute_combined_odds_fp(&[p], 1, 500, CORRELATION_MAX_BPS).unwrap();
        // Raw odds = 2.0, with 5% margin = 2.0 * 0.95 = 1.9
        let expected_fp = (1.9 * SCALE as f64) as u64;
        assert!(
            (odds as i64 - expected_fp as i64).unsigned_abs() < SCALE / 100,
            "Expected ~1.9 ({}), got {}",
            expected_fp,
            odds
        );
    }

    #[test]
    fn test_combined_odds_fp_two_legs_with_margin() {
        // Two legs at 50% each, 5% margin per leg
        let p = SCALE / 2;
        let odds = compute_combined_odds_fp(&[p, p], 2, 500, CORRELATION_MAX_BPS).unwrap();
        // Raw combined = 4.0, with 5% margin per leg = 4.0 * 0.95^2 = 3.61
        let expected_fp = (3.61 * SCALE as f64) as u64;
        assert!(
            (odds as i64 - expected_fp as i64).unsigned_abs() < SCALE / 50,
            "Expected ~3.61 ({}), got {}",
            expected_fp,
            odds
        );
    }


    fn fp_bps(bps: u64) -> u64 {
        (SCALE as u128 * bps as u128 / 10_000u128) as u64
    }

    #[test]
    fn test_same_game_logical_impossible_combo_rejected() {
        // 1 and X cannot both happen in the same full-time result market.
        let home = LogicalOutcome {
            market_index: 0,
            outcome_id: 0,
            state_mask: 0b001,
        };
        let draw = LogicalOutcome {
            market_index: 0,
            outcome_id: 1,
            state_mask: 0b010,
        };
        let state_probabilities = [fp_bps(4500), fp_bps(2800), fp_bps(2700)];

        let result = compute_joint_probability_fp(&[home, draw], &state_probabilities, 3);

        assert!(result.is_err());
    }

    #[test]
    fn test_same_game_redundant_combo_collapses_to_stricter_pick() {
        // 1 + 1X is logically just 1.
        let home = LogicalOutcome {
            market_index: 0,
            outcome_id: 0,
            state_mask: 0b001,
        };
        let home_or_draw = LogicalOutcome {
            market_index: 1,
            outcome_id: 0,
            state_mask: 0b011,
        };
        let state_probabilities = [fp_bps(4500), fp_bps(2800), fp_bps(2700)];

        let joint =
            compute_joint_probability_fp(&[home, home_or_draw], &state_probabilities, 3).unwrap();

        assert_eq!(joint, state_probabilities[0]);
    }

    #[test]
    fn test_same_game_double_chance_overlap_collapses_to_draw() {
        // 1X + X2 overlaps only at X.
        let home_or_draw = LogicalOutcome {
            market_index: 1,
            outcome_id: 0,
            state_mask: 0b011,
        };
        let draw_or_away = LogicalOutcome {
            market_index: 1,
            outcome_id: 1,
            state_mask: 0b110,
        };
        let state_probabilities = [fp_bps(4500), fp_bps(2800), fp_bps(2700)];

        let joint =
            compute_joint_probability_fp(&[home_or_draw, draw_or_away], &state_probabilities, 3)
                .unwrap();

        assert_eq!(joint, state_probabilities[1]);
    }

    #[test]
    fn test_same_game_odds_use_joint_probability_not_independent_product() {
        // State model:
        // 0 = GG + O2.5, 1 = GG + U2.5, 2 = NG + O2.5, 3 = NG + U2.5
        // GG is 55%, O2.5 is 60%, but GG AND O2.5 is 45%.
        // Naive independent odds would price the combo at 1 / 33%.
        // Same-game odds correctly price it at 1 / 45%.
        let gg = LogicalOutcome {
            market_index: 2,
            outcome_id: 0,
            state_mask: 0b0011,
        };
        let over_2_5 = LogicalOutcome {
            market_index: 3,
            outcome_id: 0,
            state_mask: 0b0101,
        };
        let state_probabilities = [fp_bps(4500), fp_bps(1000), fp_bps(1500), fp_bps(3000)];

        let same_game_odds = compute_same_game_combined_odds_fp(
            &[gg, over_2_5],
            &state_probabilities,
            4,
            0,
            CORRELATION_MAX_BPS,
            CORRELATION_MAX_BPS,
        )
        .unwrap();

        let naive_independent_odds =
            compute_combined_odds_fp(&[fp_bps(5500), fp_bps(6000)], 2, 0, CORRELATION_MAX_BPS)
                .unwrap();

        assert!(same_game_odds < naive_independent_odds);
    }

    #[test]
    fn test_same_game_statistical_discount_reduces_odds() {
        let gg = LogicalOutcome {
            market_index: 2,
            outcome_id: 0,
            state_mask: 0b0011,
        };
        let over_2_5 = LogicalOutcome {
            market_index: 3,
            outcome_id: 0,
            state_mask: 0b0101,
        };
        let state_probabilities = [fp_bps(4500), fp_bps(1000), fp_bps(1500), fp_bps(3000)];

        let full_odds = compute_same_game_combined_odds_fp(
            &[gg, over_2_5],
            &state_probabilities,
            4,
            0,
            CORRELATION_MAX_BPS,
            CORRELATION_MAX_BPS,
        )
        .unwrap();
        let discounted_odds = compute_same_game_combined_odds_fp(
            &[gg, over_2_5],
            &state_probabilities,
            4,
            0,
            CORRELATION_MAX_BPS,
            9_000,
        )
        .unwrap();

        assert_eq!(discounted_odds, full_odds * 9 / 10);
    }

    #[derive(Clone, Copy)]
    struct SimBet {
        masks: [u64; 4],
        num_legs: usize,
        stake: u64,
        payout: u64,
        joint_mask: u64,
    }

    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            self.0
        }

        fn range(&mut self, min: u64, max: u64) -> u64 {
            min + (self.next_u64() % (max - min + 1))
        }
    }

    #[test]
    fn test_simulated_same_game_users_from_open_to_settlement() {
        // Four-state football toy model:
        // 0 = home + GG + O2.5
        // 1 = home + NG + U2.5
        // 2 = draw + GG + U2.5
        // 3 = away + NG + O2.5
        let state_probabilities = [fp_bps(3200), fp_bps(1800), fp_bps(2500), fp_bps(2500)];
        let outcomes = [
            LogicalOutcome {
                market_index: 0,
                outcome_id: 0,
                state_mask: 0b0011,
            }, // 1
            LogicalOutcome {
                market_index: 0,
                outcome_id: 1,
                state_mask: 0b0100,
            }, // X
            LogicalOutcome {
                market_index: 0,
                outcome_id: 2,
                state_mask: 0b1000,
            }, // 2
            LogicalOutcome {
                market_index: 1,
                outcome_id: 0,
                state_mask: 0b0111,
            }, // 1X
            LogicalOutcome {
                market_index: 1,
                outcome_id: 1,
                state_mask: 0b1100,
            }, // X2
            LogicalOutcome {
                market_index: 1,
                outcome_id: 2,
                state_mask: 0b1011,
            }, // 12
            LogicalOutcome {
                market_index: 2,
                outcome_id: 0,
                state_mask: 0b0101,
            }, // GG
            LogicalOutcome {
                market_index: 2,
                outcome_id: 1,
                state_mask: 0b1010,
            }, // NG
            LogicalOutcome {
                market_index: 3,
                outcome_id: 0,
                state_mask: 0b1001,
            }, // O2.5
            LogicalOutcome {
                market_index: 3,
                outcome_id: 1,
                state_mask: 0b0110,
            }, // U2.5
        ];

        for seed in 1..=16u64 {
            let mut rng = Lcg::new(seed);
            let user_count = rng.range(5, 100) as usize;
            let mut treasury = 1_000_000_000u64;
            let mut locked_bonus = 0u64;
            let mut bets: Vec<SimBet> = Vec::with_capacity(user_count);

            for _ in 0..user_count {
                let num_legs = rng.range(1, 4) as usize;
                let stake = rng.range(1_000_000, 25_000_000);
                let mut selected = [LogicalOutcome::default(); 4];
                let mut masks = [0u64; 4];

                for leg_idx in 0..num_legs {
                    let outcome = outcomes[rng.range(0, (outcomes.len() - 1) as u64) as usize];
                    selected[leg_idx] = outcome;
                    masks[leg_idx] = outcome.state_mask;
                }

                let joint =
                    compute_joint_probability_fp(&selected[..num_legs], &state_probabilities, 4);

                if joint.is_err() {
                    continue;
                }

                let odds = compute_same_game_combined_odds_fp(
                    &selected[..num_legs],
                    &state_probabilities,
                    4,
                    500,
                    CORRELATION_MAX_BPS,
                    9_500,
                )
                .unwrap();
                let payout = ((stake as u128 * odds as u128) / SCALE as u128) as u64;
                let bonus_gap = payout.saturating_sub(stake);
                let free = treasury.saturating_sub(locked_bonus);
                if free < bonus_gap {
                    continue;
                }

                let mut joint_mask = 0b1111u64;
                for leg_idx in 0..num_legs {
                    joint_mask &= masks[leg_idx];
                }

                treasury = treasury.checked_add(stake).unwrap();
                locked_bonus = locked_bonus.checked_add(bonus_gap).unwrap();
                bets.push(SimBet {
                    masks,
                    num_legs,
                    stake,
                    payout,
                    joint_mask,
                });
            }

            // Settlement: deterministic final state selected after betting closes.
            let winning_state = rng.range(0, 3) as usize;
            let winning_bit = 1u64 << winning_state;
            let mut total_payout = 0u64;

            for bet in bets {
                let won = (bet.joint_mask & winning_bit) != 0;
                let bonus_gap = bet.payout.saturating_sub(bet.stake);
                locked_bonus = locked_bonus.saturating_sub(bonus_gap);

                // Sanity-check the stored masks agree with the compact joint mask.
                let mut recomputed = 0b1111u64;
                for leg_idx in 0..bet.num_legs {
                    recomputed &= bet.masks[leg_idx];
                }
                assert_eq!(recomputed, bet.joint_mask);

                if won {
                    total_payout = total_payout.checked_add(bet.payout).unwrap();
                    treasury = treasury.checked_sub(bet.payout).unwrap();
                }
            }

            assert_eq!(locked_bonus, 0);
            assert!(
                treasury <= 1_000_000_000 + (user_count as u64 * 25_000_000),
                "treasury grew beyond possible stakes"
            );
            assert!(
                total_payout <= 1_000_000_000 + (user_count as u64 * 25_000_000),
                "payouts exceeded initial liquidity plus all stakes"
            );
        }
    }
}
