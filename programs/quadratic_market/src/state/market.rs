use crate::constants::MAX_OUTCOMES;
use crate::errors::QuadraticMarketError;
use anchor_lang::prelude::*;

/// Market status - simplified for fixed odds sports betting
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum MarketStatus {
    Open,           // Accepting bets
    Suspended,      // No new bets, existing bets remain
    AwaitingResult, // Match started, awaiting oracle
    Proposed,       // Oracle proposed result, challenge window open
    Settled,        // Result finalized
    Voided,         // Market voided (no result)
}

impl Default for MarketStatus {
    fn default() -> Self {
        MarketStatus::Open
    }
}

impl MarketStatus {
    pub fn is_tradable(&self) -> bool {
        matches!(self, MarketStatus::Open)
    }

    pub fn can_settle(&self) -> bool {
        matches!(
            self,
            MarketStatus::Open | MarketStatus::Suspended | MarketStatus::AwaitingResult
        )
    }
}

/// Market type for categorization
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug, Default)]
pub enum MarketType {
    #[default]
    OneXTwo, // 1X2 market: Home(0), Draw(1), Away(2)
    OverUnder,  // Over/Under: Over(0), Under(1)
    GoalNoGoal, // GG/NG: Yes/GG(0), No/NG(1)
}

/// Simplified Market for fixed odds sports betting
/// Odds are stored as basis points (e.g., 200 = 2.00x payout)
#[account]
pub struct Market {
    pub market_id: u64,       // 8
    pub creator: Pubkey,      // 32
    pub start_time: i64,      // 8
    pub status: MarketStatus, // 1
    pub num_outcomes: u8,     // 1
    /// Fixed odds per outcome in basis points (10000 = 1.0x, 20000 = 2.0x)
    /// For 1X2: [home_odds, draw_odds, away_odds]
    /// For O/U: [over_odds, under_odds]
    /// For GG/NG: [gg_odds, ng_odds]
    pub odds: [u64; MAX_OUTCOMES], // 64
    pub exposure: u64,        // 8  — total liability for this market
    pub settlement_time: i64, // 8
    pub winning_outcome: u8,  // 1
    pub outcome_mints: [Pubkey; MAX_OUTCOMES], // 256
    pub title: String,        // 4 + 128
    pub description: String,  // 4 + 256
    pub market_type: MarketType, // 1 (enum tag)
    pub category: u8,         // 1
    pub bump: u8,             // 1
    // Optional market group association (for tracking purposes only)
    pub group_id: Option<u64>, // 9 (1 tag + 8 value)
    // Epoch tracking
    pub epoch_id: u64,          // 8 — epoch this market belongs to
    pub settled_in_epoch: bool, // 1 — true when market settlement is counted in epoch
    // TxLINE fixture reference for verifiable settlement
    pub txline_fixture_id: Option<u64>, // 9 (1 tag + 8 value) - txodds/txline fixture ID
    pub txline_proof_verified: bool,    // 1 — true when settled via on-chain TxLINE proof
}

impl Market {
    pub const LEN: usize = 8  // discriminator
        + 8   // market_id
        + 32  // creator
        + 8   // start_time
        + 1   // status
        + 1   // num_outcomes
        + 64  // odds
        + 8   // exposure
        + 8   // settlement_time
        + 1   // winning_outcome
        + 256 // outcome_mints
        + (4 + 128) // title
        + (4 + 256) // description
        + 1   // market_type
        + 1   // category
        + 1   // bump
        + 9   // group_id (Option<u64>: 1 tag + 8 value)
        + 8   // epoch_id
        + 1   // settled_in_epoch
        + 9   // txline_fixture_id (Option<u64>: 1 tag + 8 value)
        + 1   // txline_proof_verified
        + 7; // padding to align to 8

    /// Returns true if the market is currently open for trading.
    pub fn is_tradable(&self, now: i64) -> bool {
        self.status == MarketStatus::Open && now < self.start_time
    }

    /// Returns true if the market has expired (event has started).
    pub fn is_expired(&self, now: i64) -> bool {
        now >= self.start_time
    }

    /// Returns true if the market is in a state that can accept bets.
    pub fn can_place_bet(&self, now: i64) -> bool {
        self.status == MarketStatus::Open && now < self.start_time
    }

    /// Returns true if the market can be settled.
    pub fn can_settle(&self) -> bool {
        matches!(
            self.status,
            MarketStatus::Open | MarketStatus::Suspended | MarketStatus::AwaitingResult
        )
    }

    /// Returns true if the market has been finalized (settled or voided).
    pub fn is_finalized(&self) -> bool {
        matches!(self.status, MarketStatus::Settled | MarketStatus::Voided)
    }

    /// Returns the time until the market starts (negative if already started).
    pub fn time_until_start(&self, now: i64) -> i64 {
        self.start_time.saturating_sub(now)
    }

    /// Returns true if the given outcome ID is valid for this market.
    pub fn is_valid_outcome(&self, outcome_id: u8) -> bool {
        (outcome_id as usize) < self.num_outcomes as usize
    }

    /// Get the fixed odds for a specific outcome (in basis points)
    pub fn get_odds(&self, outcome_id: u8) -> Result<u64> {
        if (outcome_id as usize) >= self.num_outcomes as usize {
            return Err(QuadraticMarketError::InvalidOutcomeId.into());
        }
        Ok(self.odds[outcome_id as usize])
    }

    /// Calculate payout for a given stake on an outcome
    /// Returns payout = stake * odds_bps / 10000
    pub fn calculate_payout(&self, outcome_id: u8, stake: u64) -> Result<u64> {
        let odds = self.get_odds(outcome_id)?;
        let payout = stake
            .checked_mul(odds)
            .ok_or(QuadraticMarketError::MathOverflow)?
            / 10000;
        Ok(payout)
    }
}
