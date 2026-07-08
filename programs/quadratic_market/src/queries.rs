/// Query functions for frontend integration
/// 
/// These are read-only view functions that calculate costs, odds, and stats
/// without modifying state. They replicate protocol logic for client preview.

use anchor_lang::prelude::*;
use crate::state::{GlobalConfig, Market};
use crate::math::lmsr;
use crate::constants::*;
use crate::errors::QuadraticMarketError;

// ═══════════════════════════════════════════════════════════════════════════
// Result Structures
// ═══════════════════════════════════════════════════════════════════════════

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct QuoteBuyResult {
    pub cost: u64,              // LMSR cost before fee
    pub fee: u64,               // Buy fee (1% of cost)
    pub total_payment: u64,     // cost + fee
    pub new_q_values: Vec<u64>, // Resulting q_values after purchase
    pub new_odds: Vec<u64>,     // Decimal odds * 10000 (e.g., 25000 = 2.5x)
    pub price_impact_bps: u64,  // Price impact in basis points
    pub shares_received: u64,   // Outcome tokens minted
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct QuoteSellResult {
    pub proceeds: u64,          // LMSR proceeds before fee
    pub fee: u64,               // Sell fee (1% of proceeds)
    pub net_received: u64,      // proceeds - fee
    pub new_q_values: Vec<u64>, // Resulting q_values after sale
    pub new_odds: Vec<u64>,     // Decimal odds * 10000
    pub price_impact_bps: u64,  // Price impact in basis points
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct MarketStatsResult {
    pub market_id: u64,
    pub status: u8,                 // 0=PreOpen, 1=Open, 2=Suspended, etc.
    pub current_odds: Vec<u64>,     // Decimal odds * 10000
    pub implied_probs: Vec<u64>,    // Probabilities * 10000
    pub total_volume: u64,          // Sum of all q_values
    pub liquidity: u64,             // market.backing
    pub exposure: u64,              // market.exposure
    pub locked_payout: u64,         // Outstanding liability
    pub num_outcomes: u8,
    pub time_to_close: i64,         // Seconds until start_time (negative if closed)
    pub time_to_settlement: i64,    // Seconds until can settle
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct LpStatsResult {
    pub total_tvl: u64,             // Treasury balance
    pub total_lp_supply: u64,       // Total LP tokens minted
    pub locked_exposure: u64,       // Locked for outstanding bets
    pub free_liquidity: u64,        // Available for new bets
    pub nav_per_share: u64,         // Net asset value per LP token (scaled by 1e6)
    pub total_markets: u64,
    pub active_markets: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// Quote Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Quote cost for buying outcome shares
pub fn quote_buy(
    market: &Market,
    outcome_id: u8,
    num_shares: u64,
) -> Result<QuoteBuyResult> {
    require!(
        outcome_id < market.num_outcomes,
        QuadraticMarketError::InvalidOutcomeId
    );
    require!(num_shares > 0, QuadraticMarketError::InvalidAmount);

    // Calculate cost using LMSR
    let cost = lmsr::lmsr_buy_cost(
        &market.q_values,
        market.num_outcomes,
        outcome_id,
        num_shares,
        market.lmsr_b,
    )?;

    // Calculate fee (1%)
    let fee = cost / 100;
    let total_payment = cost.checked_add(fee).unwrap();

    // Calculate new q_values
    let mut new_q_values = market.q_values.clone();
    new_q_values[outcome_id as usize] = new_q_values[outcome_id as usize]
        .checked_add(num_shares)
        .unwrap();

    // Calculate new odds (decimal * 10000)
    let new_odds = calculate_decimal_odds(&new_q_values, market.num_outcomes as usize)?;

    // Calculate price impact
    let old_odds = calculate_decimal_odds(&market.q_values, market.num_outcomes as usize)?;
    let old_price = old_odds[outcome_id as usize];
    let new_price = new_odds[outcome_id as usize];
    let price_impact_bps = if old_price > 0 {
        (((old_price as i64 - new_price as i64).abs() as u64) * 10000) / old_price
    } else {
        0
    };

    Ok(QuoteBuyResult {
        cost,
        fee,
        total_payment,
        new_q_values: new_q_values.to_vec(),
        new_odds,
        price_impact_bps,
        shares_received: num_shares,
    })
}

/// Quote proceeds for selling outcome shares
pub fn quote_sell(
    market: &Market,
    outcome_id: u8,
    num_shares: u64,
) -> Result<QuoteSellResult> {
    require!(
        outcome_id < market.num_outcomes,
        QuadraticMarketError::InvalidOutcomeId
    );
    require!(num_shares > 0, QuadraticMarketError::InvalidAmount);
    require!(
        market.q_values[outcome_id as usize] >= num_shares,
        QuadraticMarketError::InsufficientShares
    );

    // Calculate proceeds using LMSR
    let proceeds = lmsr::lmsr_sell_payout(
        &market.q_values,
        market.num_outcomes,
        outcome_id,
        num_shares,
        market.lmsr_b,
    )?;

    // Calculate fee (1%)
    let fee = proceeds / 100;
    let net_received = proceeds.saturating_sub(fee);

    // Calculate new q_values
    let mut new_q_values = market.q_values.clone();
    new_q_values[outcome_id as usize] = new_q_values[outcome_id as usize]
        .saturating_sub(num_shares);

    // Calculate new odds
    let new_odds = calculate_decimal_odds(&new_q_values, market.num_outcomes as usize)?;

    // Calculate price impact
    let old_odds = calculate_decimal_odds(&market.q_values, market.num_outcomes as usize)?;
    let old_price = old_odds[outcome_id as usize];
    let new_price = new_odds[outcome_id as usize];
    let price_impact_bps = if old_price > 0 {
        (((new_price as i64 - old_price as i64).abs() as u64) * 10000) / old_price
    } else {
        0
    };

    Ok(QuoteSellResult {
        proceeds,
        fee,
        net_received,
        new_q_values: new_q_values.to_vec(),
        new_odds,
        price_impact_bps,
    })
}


// ═══════════════════════════════════════════════════════════════════════════
// Market Stats Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Get comprehensive market statistics
pub fn get_market_stats(market: &Market, current_time: i64) -> Result<MarketStatsResult> {
    // Calculate current odds
    let current_odds = calculate_decimal_odds(&market.q_values, market.num_outcomes as usize)?;
    
    // Calculate implied probabilities (inverse of odds, normalized)
    let implied_probs = calculate_implied_probabilities(&current_odds)?;

    // Calculate total volume (sum of q_values)
    let total_volume = market.q_values[0..market.num_outcomes as usize]
        .iter()
        .sum();

    // Time calculations
    let time_to_close = market.start_time - current_time;
    let time_to_settlement = market.settlement_time - current_time;

    // Market status as u8
    let status = match market.status {
        crate::state::market::MarketStatus::PreOpen => 0,
        crate::state::market::MarketStatus::Open => 1,
        crate::state::market::MarketStatus::Suspended => 2,
        crate::state::market::MarketStatus::AwaitingResult => 3,
        crate::state::market::MarketStatus::Proposed => 4,
        crate::state::market::MarketStatus::Settled => 5,
        crate::state::market::MarketStatus::Voided => 6,
    };

    Ok(MarketStatsResult {
        market_id: market.market_id,
        status,
        current_odds,
        implied_probs,
        total_volume,
        liquidity: market.backing,
        exposure: market.exposure,
        locked_payout: market.locked_payout,
        num_outcomes: market.num_outcomes,
        time_to_close,
        time_to_settlement,
    })
}

/// Get LP pool statistics
pub fn get_lp_stats(
    config: &GlobalConfig,
    treasury_balance: u64,
) -> Result<LpStatsResult> {
    let free_liquidity = config.free_liquidity(treasury_balance);
    
    // NAV per share = treasury_balance / total_lp_supply (scaled by 1e6)
    let nav_per_share = if config.total_lp_supply > 0 {
        (treasury_balance as u128 * 1_000_000 / config.total_lp_supply as u128) as u64
    } else {
        1_000_000 // 1.0 if no supply
    };

    Ok(LpStatsResult {
        total_tvl: treasury_balance,
        total_lp_supply: config.total_lp_supply,
        locked_exposure: config.locked_payouts,
        free_liquidity,
        nav_per_share,
        total_markets: 0,      // TODO: Track on-chain
        active_markets: 0,     // TODO: Track on-chain
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Calculate decimal odds from q_values
/// Returns odds * 10000 (e.g., 25000 = 2.5x)
fn calculate_decimal_odds(q_values: &[u64; MAX_OUTCOMES], num_outcomes: usize) -> Result<Vec<u64>> {
    let mut odds = Vec::with_capacity(num_outcomes);
    let total: u64 = q_values[0..num_outcomes].iter().sum();

    if total == 0 {
        return err!(QuadraticMarketError::MathOverflow);
    }

    for i in 0..num_outcomes {
        let q = q_values[i];
        if q == 0 {
            odds.push(0);
        } else {
            // decimal_odds = total / q
            // Scaled by 10000: (total * 10000) / q
            let decimal_odds = (total as u128 * 10000 / q as u128) as u64;
            odds.push(decimal_odds);
        }
    }

    Ok(odds)
}

/// Calculate implied probabilities from decimal odds
/// Returns probabilities * 10000 (e.g., 2500 = 25%)
fn calculate_implied_probabilities(decimal_odds: &[u64]) -> Result<Vec<u64>> {
    let mut probs = Vec::with_capacity(decimal_odds.len());
    let mut total_prob: u128 = 0;

    for &odds in decimal_odds {
        if odds == 0 {
            probs.push(0);
        } else {
            // implied_prob = 1 / odds
            // Scaled: (10000 * 10000) / odds
            let prob = (10000 * 10000) / odds as u128;
            probs.push(prob as u64);
            total_prob += prob;
        }
    }

    // Normalize probabilities to sum to 10000 (100%)
    if total_prob > 0 {
        for prob in probs.iter_mut() {
            *prob = (*prob as u128 * 10000 / total_prob) as u64;
        }
    }

    Ok(probs)
}
