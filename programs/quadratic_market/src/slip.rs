use crate::constants::{seeds, MAX_SLIP_LEGS};
use crate::errors::QuadraticMarketError;
use crate::state::{
    market_group::CORRELATION_BPS_MULTIPLIER,
    GlobalConfig, Market, MarketGroup, MarketStatus, SlipLeg,
};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// ─── Slip Status ─────────────────────────────────────────────────

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum SlipStatus {
    /// Slip created, awaiting legs to be bought
    Pending,
    /// All legs bought, awaiting settlement
    Active,
    /// All legs settled, all won
    Won,
    /// All legs settled, at least one lost
    Lost,
    /// Cancelled before completion
    Cancelled,
}

impl Default for SlipStatus {
    fn default() -> Self {
        SlipStatus::Pending
    }
}

// ─── New Slip State ──────────────────────────────────────────────

#[account]
pub struct Slip {
    pub owner: Pubkey,
    pub slip_id: u64,
    pub epoch_id: u64,
    pub num_legs: u8,
    pub leg_market_ids: [u64; MAX_SLIP_LEGS],
    pub leg_outcome_ids: [u8; MAX_SLIP_LEGS],
    pub legs_bought_mask: u16,                    // bit i = leg i bought
    pub legs_settled_mask: u16,                   // bit i = leg i settled
    pub legs_won_mask: u16,                       // bit i = leg i won
    pub total_stake: u64,                         // Total USDC escrowed
    pub total_cost: u64,                          // Sum of actual leg costs (for payout calc)
    pub potential_payout: u64,                    // Fixed payout if all legs win
    pub locked_amount: u64,                       // Current treasury lock
    pub status: SlipStatus,
    pub created_at: i64,
    pub cancel_deadline: i64,
    pub claimed: bool, // Whether paused bet has been reclaimed
    pub bump: u8,
}

impl Slip {
    pub const LEN: usize = 8   // discriminator
        + 32  // owner
        + 8   // slip_id
        + 8   // epoch_id
        + 1   // num_legs
        + 40  // leg_market_ids (5 * 8)
        + 5   // leg_outcome_ids (5 * 1)
        + 2   // legs_bought_mask
        + 2   // legs_settled_mask
        + 2   // legs_won_mask
        + 8   // total_stake
        + 8   // total_cost
        + 8   // potential_payout
        + 8   // locked_amount
        + 1   // status
        + 8   // created_at
        + 8   // cancel_deadline
        + 1   // claimed
        + 1; // bump

    /// Check if all legs have been bought
    pub fn all_legs_bought(&self) -> bool {
        let expected = ((1u16 << self.num_legs) - 1) as u16;
        self.legs_bought_mask == expected
    }

    /// Check if all legs have been settled
    pub fn all_legs_settled(&self) -> bool {
        let expected = ((1u16 << self.num_legs) - 1) as u16;
        self.legs_settled_mask == expected
    }

    /// Check if all legs won
    pub fn all_legs_won(&self) -> bool {
        let expected = ((1u16 << self.num_legs) - 1) as u16;
        self.legs_won_mask == expected
    }

    /// Returns true if the slip can be cancelled (deadline passed or not all legs bought).
    pub fn can_cancel(&self, now: i64) -> bool {
        matches!(self.status, SlipStatus::Pending | SlipStatus::Active)
            && now >= self.cancel_deadline
            && !self.all_legs_bought()
    }

    /// Returns the number of legs remaining to be bought.
    pub fn legs_remaining(&self) -> u8 {
        self.legs_bought_mask.count_ones() as u8;
        self.num_legs - (self.legs_bought_mask.count_ones() as u8)
    }

    /// Returns the number of legs that have been bought.
    pub fn legs_bought_count(&self) -> u8 {
        self.legs_bought_mask.count_ones() as u8
    }

    /// Returns the number of legs that have been settled.
    pub fn legs_settled_count(&self) -> u8 {
        self.legs_settled_mask.count_ones() as u8
    }

    /// Returns the number of legs that won.
    pub fn legs_won_count(&self) -> u8 {
        self.legs_won_mask.count_ones() as u8
    }

    /// Returns the number of legs that lost.
    pub fn legs_lost_count(&self) -> u8 {
        let settled = self.legs_settled_count();
        let won = self.legs_won_count();
        settled.saturating_sub(won)
    }

    /// Check if a specific leg has been bought.
    pub fn is_leg_bought(&self, leg_index: u8) -> bool {
        if leg_index >= self.num_legs {
            return false;
        }
        self.legs_bought_mask & (1u16 << leg_index) != 0
    }

    /// Check if a specific leg has been settled.
    pub fn is_leg_settled(&self, leg_index: u8) -> bool {
        if leg_index >= self.num_legs {
            return false;
        }
        self.legs_settled_mask & (1u16 << leg_index) != 0
    }

    /// Check if a specific leg won.
    pub fn is_leg_won(&self, leg_index: u8) -> bool {
        if leg_index >= self.num_legs {
            return false;
        }
        self.legs_won_mask & (1u16 << leg_index) != 0
    }

    /// Returns true if the slip is in a final state (won, lost, or cancelled).
    pub fn is_finalized(&self) -> bool {
        matches!(
            self.status,
            SlipStatus::Won | SlipStatus::Lost | SlipStatus::Cancelled
        )
    }

    /// Returns the time remaining until the cancel deadline (negative if passed).
    pub fn time_until_cancel_deadline(&self, now: i64) -> i64 {
        self.cancel_deadline.saturating_sub(now)
    }
}

// ─── Correlation Validation Helpers ────────────────────────────────────────────

/// Validate slip legs for correlation and mutual exclusivity.
/// Returns error if:
/// - Same market, different outcomes (mutually exclusive)
/// - Legs from different groups are independent (no correlation applied)
pub fn validate_slip_correlation(
    legs: &[SlipLeg],
    _markets: &[&Account<Market>],
    group: Option<&Account<MarketGroup>>,
) -> Result<u64> {
    let n = legs.len();
    require!(n > 0, QuadraticMarketError::SlipNoLegs);
    require!(n <= MAX_SLIP_LEGS, QuadraticMarketError::SlipTooManyLegs);

    // Build market index mapping: market_id -> group market index (0=1X2, 1=O/U, 2=GGNG)
    let mut market_to_group_index: std::collections::HashMap<u64, usize> =
        std::collections::HashMap::new();
    if let Some(g) = group {
        for (idx, &mkt_id) in g.market_ids.iter().enumerate() {
            if mkt_id != 0 {
                market_to_group_index.insert(mkt_id, idx);
            }
        }
    }

    // Check for same-market, different outcomes
    let mut seen_markets: std::collections::HashMap<u64, u8> = std::collections::HashMap::new();
    for leg in legs {
        if let Some(&prev_outcome) = seen_markets.get(&leg.market_id) {
            // Same market, different outcome = MUTUALLY EXCLUSIVE = REJECT
            require!(
                prev_outcome == leg.outcome_id,
                QuadraticMarketError::CorrelatedLegsMutuallyExclusive
            );
        } else {
            seen_markets.insert(leg.market_id, leg.outcome_id);
        }
    }

    // Calculate correlation-adjusted payout multiplier
    // For each pair of legs from the same group, apply correlation
    let mut total_multiplier = 10000u64; // Start at 1.0x (10000 bps)

    if let Some(g) = group {
        let correlations = &g.correlation_matrix;

        // For each pair of legs
        for i in 0..n {
            for j in (i + 1)..n {
                // Get group indices
                let Some(idx_i) = market_to_group_index.get(&legs[i].market_id) else {
                    continue;
                };
                let Some(idx_j) = market_to_group_index.get(&legs[j].market_id) else {
                    continue;
                };

                // Same group index = same market type, calculate correlation
                if idx_i == idx_j {
                    // Different markets of same type in group - this shouldn't happen with proper market creation
                    continue;
                }

                // Get correlation score
                let corr_bps = correlations.get_correlation(*idx_i, *idx_j);

                // Apply formula: multiplier = 1 - (correlation_bps * 25 / 10000)
                // We multiply the total multiplier by this
                let pair_multiplier =
                    10000u64.saturating_sub(corr_bps as u64 * CORRELATION_BPS_MULTIPLIER);

                // Accumulate correlation effect
                // Simple approach: multiply all pair multipliers together (in bps)
                total_multiplier = total_multiplier
                    .saturating_mul(pair_multiplier)
                    .saturating_div(10000); // Back to bps
            }
        }
    }

    Ok(total_multiplier) // Returns multiplier in bps (10000 = no correlation discount)
}

/// Calculate potential payout for a slip with correlation adjustment.
///
/// payout = sum(leg_payouts) * correlation_multiplier
///
/// Where leg_payout = stake_per_leg * odds_bps / 10000
pub fn calculate_correlated_payout(
    legs: &[SlipLeg],
    fixed_odds_bps: &[u64],
    total_stake: u64,
    correlation_multiplier_bps: u64,
) -> Result<u64> {
    let n = legs.len() as u64;
    require!(n > 0, QuadraticMarketError::SlipNoLegs);

    let stake_per_leg = total_stake
        .checked_div(n)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Calculate sum of individual leg payouts
    let mut sum_payouts = 0u64;
    for i in 0..legs.len() {
        let payout = stake_per_leg
            .checked_mul(fixed_odds_bps[i])
            .ok_or(QuadraticMarketError::MathOverflow)?
            .checked_div(10000)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        sum_payouts = sum_payouts
            .checked_add(payout)
            .ok_or(QuadraticMarketError::MathOverflow)?;
    }

    // Apply correlation multiplier
    // Note: For independent legs, multiplier = 10000 (1.0)
    // For fully correlated, multiplier = 7500 (0.75)
    let correlated_payout = sum_payouts
        .checked_mul(correlation_multiplier_bps)
        .ok_or(QuadraticMarketError::MathOverflow)?
        .checked_div(10000)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    Ok(correlated_payout)
}

// ─── Place Slip Await ───────────────────────────────────────────
// User escrows stake and records legs.
// Backend then fires N separate buy_leg_for_slip transactions.

#[derive(Accounts)]
pub struct PlaceSlipAwait<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        init,
        payer = owner,
        space = Slip::LEN,
        seeds = [seeds::SLIP, global_config.next_slip_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub slip: Account<'info, Slip>,

    /// CHECK: Treasury PDA for escrow
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    #[account(mut)]
    pub owner_base_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    #[account(mut)]
    pub owner: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn place_slip_await_handler<'info>(
    ctx: Context<'_, '_, '_, 'info, PlaceSlipAwait<'info>>,
    legs: Vec<SlipLeg>,
    stake: u64,
    cancel_deadline: i64,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let slip = &mut ctx.accounts.slip;

    require!(!config.paused, QuadraticMarketError::Paused);
    require!(legs.len() > 0, QuadraticMarketError::SlipNoLegs);
    require!(
        legs.len() <= MAX_SLIP_LEGS,
        QuadraticMarketError::SlipTooManyLegs
    );
    require!(stake > 0, QuadraticMarketError::InvalidAmount);
    // ─── Same-Market Rejection Check ───────────────────────────────
    // Cannot bet two different outcomes from the same market
    // e.g., Home AND Away from same 1X2 market is MUTUALLY EXCLUSIVE
    {
        use std::collections::HashMap;
        let mut seen: HashMap<u64, u8> = HashMap::new();
        for leg in &legs {
            if let Some(&prev_outcome) = seen.get(&leg.market_id) {
                // Same market_id with different outcome_id = REJECT
                require!(
                    prev_outcome == leg.outcome_id,
                    QuadraticMarketError::CorrelatedLegsMutuallyExclusive
                );
            } else {
                seen.insert(leg.market_id, leg.outcome_id);
            }
        }
    }

    // Get slip_id from counter
    let slip_id = config.next_slip_id;
    config.next_slip_id = config
        .next_slip_id
        .checked_add(1)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    let now = Clock::get()?.unix_timestamp;

    // Initialize slip
    slip.owner = ctx.accounts.owner.key();
    slip.slip_id = slip_id;
    slip.epoch_id = config.current_epoch;
    slip.num_legs = legs.len() as u8;
    slip.legs_bought_mask = 0;
    slip.legs_settled_mask = 0;
    slip.legs_won_mask = 0;
    slip.total_stake = stake;
    slip.total_cost = 0; // Accumulated from leg costs during buy_leg_for_slip
    slip.potential_payout = 0; // Will be calculated after all legs bought
    slip.locked_amount = 0;
    slip.status = SlipStatus::Pending;
    slip.created_at = now;
    slip.cancel_deadline = cancel_deadline;
    slip.bump = *ctx.bumps.get("slip").unwrap();

    // Record legs and current references only.
    for (i, leg) in legs.iter().enumerate() {
        slip.leg_market_ids[i] = leg.market_id;
        slip.leg_outcome_ids[i] = leg.outcome_id;
    }

    // Initialize remaining slots to zero
    for i in legs.len()..MAX_SLIP_LEGS {
        slip.leg_market_ids[i] = 0;
        slip.leg_outcome_ids[i] = 0;
    }

    // Transfer stake to treasury escrow
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.owner_base_ata.to_account_info(),
                to: ctx.accounts.treasury_base_ata.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            },
        ),
        stake,
    )?;

    emit!(SlipAwaited {
        slip_id,
        owner: slip.owner,
        num_legs: slip.num_legs,
        stake,
        cancel_deadline,
    });

    Ok(())
}

// ─── Buy Leg For Slip ────────────────────────────────────────────
// One instruction per leg. Same account footprint as normal single bet.
// Backend calls this N times after place_slip_await.

#[derive(Accounts)]
#[instruction(slip_id: u64, leg_index: u8)]
pub struct BuyLegForSlip<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::SLIP, slip_id.to_le_bytes().as_ref()],
        bump = slip.bump,
    )]
    pub slip: Account<'info, Slip>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Box<Account<'info, Market>>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    #[account(mut)]
    pub buyer_outcome_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = outcome_mint.key() == market.outcome_mints[leg_index as usize] @ QuadraticMarketError::WrongOutcomeToken,
    )]
    pub outcome_mint: Account<'info, Mint>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub buyer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn buy_leg_for_slip_handler<'info>(
    ctx: Context<'_, '_, '_, 'info, BuyLegForSlip<'info>>,
    slip_id: u64,
    leg_index: u8,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let slip = &mut ctx.accounts.slip;
    let market = &mut ctx.accounts.market;

    // Validate slip
    require!(
        slip.status == SlipStatus::Pending || slip.status == SlipStatus::Active,
        QuadraticMarketError::InvalidMarketStatus
    );
    require!(
        leg_index < slip.num_legs,
        QuadraticMarketError::InvalidOutcomeId
    );

    // Check if leg already bought
    let bit = 1u16 << leg_index;
    require!(
        slip.legs_bought_mask & bit == 0,
        QuadraticMarketError::SlipAlreadyClaimed // Reuse error
    );

    // Validate market matches
    let expected_market_id = slip.leg_market_ids[leg_index as usize];
    require!(
        market.market_id == expected_market_id,
        QuadraticMarketError::InvalidRemainingAccount
    );

    // Validate outcome matches
    let expected_outcome = slip.leg_outcome_ids[leg_index as usize];
    require!(
        expected_outcome < market.num_outcomes,
        QuadraticMarketError::InvalidOutcomeId
    );

    // Market must be open
    require!(
        market.status == MarketStatus::Open,
        QuadraticMarketError::MarketNotOpen
    );
    require!(
        market.epoch_id == slip.epoch_id,
        QuadraticMarketError::EpochAccountMismatch
    );

    // Check cancel deadline
    let now = Clock::get()?.unix_timestamp;
    require!(
        now < slip.cancel_deadline,
        QuadraticMarketError::SlipExpired // Reuse error
    );

    // Use the current market odds at execution time.
    let fixed_odds = market.odds[expected_outcome as usize];

    // Calculate the leg's share of the total stake (handle remainder in last leg)
    let leg_stake = slip.total_stake / slip.num_legs as u64;

    // Apply house fee: fee = leg_stake * house_fee_bps / 10000
    let fee = leg_stake
        .checked_mul(config.house_fee_bps)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;

    // Net stake after fee
    let net_stake = leg_stake
        .checked_sub(fee)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Calculate potential payout for this leg: net_stake * odds_bps / 10000
    let leg_payout = net_stake
        .checked_mul(fixed_odds)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;

    // Update market exposure (liability for this leg's payout)
    market.exposure = market
        .exposure
        .checked_add(leg_payout)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Update locked_payouts for the potential payout
    config.locked_payouts = config
        .locked_payouts
        .checked_add(leg_payout)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Mark leg as bought
    slip.legs_bought_mask |= bit;

    // Accumulate the cost and potential payout
    slip.total_cost = slip
        .total_cost
        .checked_add(leg_payout)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Transition to Active if all legs bought
    if slip.all_legs_bought() {
        slip.status = SlipStatus::Active;

        // Calculate potential payout based on odds (multiplied across all legs)
        // Use net stake per leg
        let leg_net_stake = slip.total_stake / slip.num_legs as u64;
        let leg_fee = leg_net_stake * config.house_fee_bps / 10000;
        let leg_net = leg_net_stake - leg_fee;

        let mut total_payout: u64 = leg_net; // Start with net stake of first leg
        for i in 0..slip.num_legs as usize {
            let odds_bps = market.odds[slip.leg_outcome_ids[i] as usize];
            total_payout = total_payout
                .checked_mul(odds_bps)
                .ok_or(QuadraticMarketError::MathOverflow)?
                / 10000;
        }
        slip.potential_payout = total_payout;
        slip.locked_amount = total_payout;
    }

    emit!(SlipLegBought {
        slip_id,
        leg_index,
        market_id: market.market_id,
        outcome: expected_outcome,
        stake: leg_stake,
        payout: leg_payout,
    });

    Ok(())
}

// ─── Cancel Slip ─────────────────────────────────────────────────
// Permissionless cancellation if deadline passed or market suspended.

#[derive(Accounts)]
#[instruction(slip_id: u64)]
pub struct CancelSlip<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::SLIP, slip_id.to_le_bytes().as_ref()],
        bump = slip.bump,
        close = owner,
    )]
    pub slip: Account<'info, Slip>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    /// CHECK: Slip owner; receives the refund.
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = owner,
    )]
    pub canceller_base_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub fn cancel_slip_handler<'info>(
    ctx: Context<'_, '_, '_, 'info, CancelSlip<'info>>,
    slip_id: u64,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let slip = &mut ctx.accounts.slip;

    require!(
        ctx.accounts.owner.key() == slip.owner,
        QuadraticMarketError::Unauthorized
    );
    require!(
        slip.status == SlipStatus::Pending || slip.status == SlipStatus::Active,
        QuadraticMarketError::InvalidMarketStatus
    );

    let now = Clock::get()?.unix_timestamp;

    // Can cancel if deadline passed OR not all legs bought
    let deadline_passed = now >= slip.cancel_deadline;
    let not_all_bought = !slip.all_legs_bought();

    require!(
        deadline_passed || not_all_bought,
        QuadraticMarketError::SlipNotExpired // Reuse error
    );

    // Calculate refund based on actual costs incurred
    // The treasury received total_stake at place_slip_await
    // But only total_cost was used to purchase tokens (sum of leg_costs)
    // The difference (total_stake - total_cost) is always refundable
    // Plus any unused legs' proportional stake
    let legs_bought = slip.legs_bought_mask.count_ones() as u64;
    let _legs_not_bought = slip.num_legs as u64 - legs_bought;

    // Used stake = legs_bought * (total_stake / num_legs)
    let used_stake = legs_bought * (slip.total_stake / slip.num_legs as u64);
    // Refund = total_stake - used_stake (unused portion of stake)
    let refund = slip.total_stake - used_stake;

    // Release locked_payouts by total_cost for the bought legs
    // This reverses the locked_payouts increase from buy_leg_for_slip
    config.locked_payouts = config.locked_payouts.saturating_sub(slip.total_cost);

    // Mark as cancelled
    slip.status = SlipStatus::Cancelled;

    // Transfer refund to owner
    let treasury_seeds: &[&[&[u8]]] = &[&[seeds::TREASURY, &[config.treasury_bump]]];
    if refund > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.treasury_base_ata.to_account_info(),
                    to: ctx.accounts.canceller_base_ata.to_account_info(),
                    authority: ctx.accounts.treasury.to_account_info(),
                },
                treasury_seeds,
            ),
            refund,
        )?;
    }

    emit!(SlipCancelled {
        slip_id,
        owner: slip.owner,
        refund,
        legs_bought: legs_bought as u32,
    });

    Ok(())
}

// ─── Settle Slip Leg ────────────────────────────────────────────
// Permissionless. Marks one leg as settled based on market outcome.

#[derive(Accounts)]
#[instruction(slip_id: u64, leg_index: u8)]
pub struct SettleSlipLeg<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::SLIP, slip_id.to_le_bytes().as_ref()],
        bump = slip.bump,
    )]
    pub slip: Account<'info, Slip>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Box<Account<'info, Market>>,

    pub caller: Signer<'info>,
}

pub fn settle_slip_leg_handler<'info>(
    ctx: Context<'_, '_, '_, 'info, SettleSlipLeg<'info>>,
    slip_id: u64,
    leg_index: u8,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let slip = &mut ctx.accounts.slip;
    let market = &ctx.accounts.market;

    // Slip must be active
    require!(
        slip.status == SlipStatus::Active,
        QuadraticMarketError::InvalidMarketStatus
    );

    // Check leg index
    require!(
        leg_index < slip.num_legs,
        QuadraticMarketError::InvalidOutcomeId
    );

    // Check leg not already settled
    let bit = 1u16 << leg_index;
    require!(
        slip.legs_settled_mask & bit == 0,
        QuadraticMarketError::SlipAlreadyClaimed // Reuse error
    );

    // Validate market matches
    let expected_market_id = slip.leg_market_ids[leg_index as usize];
    require!(
        market.market_id == expected_market_id,
        QuadraticMarketError::InvalidRemainingAccount
    );

    // Market must be settled
    require!(
        market.status == MarketStatus::Settled,
        QuadraticMarketError::MarketNotSettled
    );

    // Determine if leg won
    let expected_outcome = slip.leg_outcome_ids[leg_index as usize];
    let won = market.winning_outcome == expected_outcome;

    // Mark leg as settled
    slip.legs_settled_mask |= bit;
    if won {
        slip.legs_won_mask |= bit;
    }

    // Release exposure from this leg
    let leg_stake = slip.total_stake / slip.num_legs as u64;
    let fee = leg_stake
        .checked_mul(config.house_fee_bps)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;
    let net_stake = leg_stake
        .checked_sub(fee)
        .ok_or(QuadraticMarketError::MathOverflow)?;
    let leg_payout = net_stake
        .checked_mul(market.odds[expected_outcome as usize])
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;
    config.locked_payouts = config.locked_payouts.saturating_sub(leg_payout);

    emit!(SlipLegSettled {
        slip_id,
        leg_index,
        market_id: market.market_id,
        outcome: expected_outcome,
        winning_outcome: market.winning_outcome,
        won,
    });

    // Check if all legs settled
    if slip.all_legs_settled() {
        if slip.all_legs_won() {
            slip.status = SlipStatus::Won;
        } else {
            slip.status = SlipStatus::Lost;
        }
    }

    Ok(())
}

// ─── Resolve Slip ───────────────────────────────────────────────
// Finalizes slip and transfers payout. Permissionless.

#[derive(Accounts)]
#[instruction(slip_id: u64)]
pub struct ResolveSlip<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::SLIP, slip_id.to_le_bytes().as_ref()],
        bump = slip.bump,
    )]
    pub slip: Account<'info, Slip>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    /// CHECK: Slip owner; receives the payout and rent.
    pub owner: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = owner,
    )]
    pub claimer_base_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub fn resolve_slip_handler<'info>(
    ctx: Context<'_, '_, '_, 'info, ResolveSlip<'info>>,
    slip_id: u64,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let slip = &ctx.accounts.slip;

    // Must be won or lost
    require!(
        slip.status == SlipStatus::Won || slip.status == SlipStatus::Lost,
        QuadraticMarketError::SlipNotSettled
    );
    require!(
        ctx.accounts.owner.key() == slip.owner,
        QuadraticMarketError::Unauthorized
    );

    let treasury_seeds: &[&[&[u8]]] = &[&[seeds::TREASURY, &[config.treasury_bump]]];

    if slip.status == SlipStatus::Won {
        // Transfer payout to winner
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.treasury_base_ata.to_account_info(),
                    to: ctx.accounts.claimer_base_ata.to_account_info(),
                    authority: ctx.accounts.treasury.to_account_info(),
                },
                treasury_seeds,
            ),
            slip.potential_payout,
        )?;

        emit!(SlipResolved {
            slip_id,
            owner: slip.owner,
            status: "won".to_string(),
            payout: slip.potential_payout,
        });
    } else {
        // Lost: payout stays in treasury (house win)
        // The original stake was already taken at place_slip_await
        emit!(SlipResolved {
            slip_id,
            owner: slip.owner,
            status: "lost".to_string(),
            payout: 0,
        });
    }

    // Release remaining locked amount
    config.locked_payouts = config.locked_payouts.saturating_sub(slip.locked_amount);

    Ok(())
}

// ─── Events ─────────────────────────────────────────────────────

#[event]
pub struct SlipAwaited {
    pub slip_id: u64,
    pub owner: Pubkey,
    pub num_legs: u8,
    pub stake: u64,
    pub cancel_deadline: i64,
}

#[event]
pub struct SlipLegBought {
    pub slip_id: u64,
    pub leg_index: u8,
    pub market_id: u64,
    pub outcome: u8,
    pub stake: u64,
    pub payout: u64,
}

#[event]
pub struct SlipCancelled {
    pub slip_id: u64,
    pub owner: Pubkey,
    pub refund: u64,
    pub legs_bought: u32,
}

#[event]
pub struct SlipLegSettled {
    pub slip_id: u64,
    pub leg_index: u8,
    pub market_id: u64,
    pub outcome: u8,
    pub winning_outcome: u8,
    pub won: bool,
}

#[event]
pub struct SlipResolved {
    pub slip_id: u64,
    pub owner: Pubkey,
    pub status: String,
    pub payout: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slip_len_matches_expected() {
        // Verify the LEN constant is correct
        // 8 + 32 + 8 + 8 + 1 + 40 + 5 + 2 + 2 + 2 + 8 + 8 + 8 + 8 + 1 + 8 + 8 + 1 + 1 = 159
        assert_eq!(Slip::LEN, 159);
    }

    #[test]
    fn all_legs_bought_mask() {
        let mut slip = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 3,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0,
            legs_settled_mask: 0,
            legs_won_mask: 0,
            total_stake: 1000,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Pending,
            created_at: 0,
            cancel_deadline: 0,
            bump: 1,
            claimed: false,
        };

        // Initially not all bought
        assert!(!slip.all_legs_bought());

        // Buy 2 legs
        slip.legs_bought_mask = 0b0111; // legs 0, 1, 2
        assert!(slip.all_legs_bought());
    }

    #[test]
    fn all_legs_won() {
        let mut slip = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 3,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0b0111,
            legs_settled_mask: 0b0111,
            legs_won_mask: 0b0111,
            total_stake: 1000,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Active,
            created_at: 0,
            cancel_deadline: 0,
            bump: 1,
            claimed: false,
        };

        assert!(slip.all_legs_won());

        // One leg lost
        slip.legs_won_mask = 0b0011;
        assert!(!slip.all_legs_won());
    }

    #[test]
    fn test_can_cancel() {
        let mut slip = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 3,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0b0001, // Only leg 0 bought
            legs_settled_mask: 0,
            legs_won_mask: 0,
            total_stake: 1000,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Pending,
            created_at: 0,
            cancel_deadline: 1000,
            bump: 1,
            claimed: false,
        };

        // Deadline not passed yet
        assert!(!slip.can_cancel(500));

        // Deadline passed, not all bought
        assert!(slip.can_cancel(1001));

        // All legs bought - can't cancel
        slip.legs_bought_mask = 0b0111;
        assert!(!slip.can_cancel(1001));
    }

    #[test]
    fn test_legs_remaining() {
        let slip = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 5,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0b0011, // legs 0 and 1 bought
            legs_settled_mask: 0,
            legs_won_mask: 0,
            total_stake: 1000,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Pending,
            created_at: 0,
            cancel_deadline: 0,
            bump: 1,
            claimed: false,
        };

        assert_eq!(slip.legs_remaining(), 3);
    }

    #[test]
    fn test_legs_count_helpers() {
        let slip = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 4,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0b0101,  // legs 0 and 2 bought
            legs_settled_mask: 0b0101, // legs 0 and 2 settled
            legs_won_mask: 0b0100,     // only leg 2 won
            total_stake: 1000,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Active,
            created_at: 0,
            cancel_deadline: 0,
            bump: 1,
            claimed: false,
        };

        assert_eq!(slip.legs_bought_count(), 2);
        assert_eq!(slip.legs_settled_count(), 2);
        assert_eq!(slip.legs_won_count(), 1);
        assert_eq!(slip.legs_lost_count(), 1);
    }

    #[test]
    fn test_is_finalized() {
        let slip_pending = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 2,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0,
            legs_settled_mask: 0,
            legs_won_mask: 0,
            total_stake: 0,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Pending,
            created_at: 0,
            cancel_deadline: 0,
            bump: 1,
            claimed: false,
        };
        assert!(!slip_pending.is_finalized());

        let slip_won = Slip {
            status: SlipStatus::Won,
            ..slip_pending
        };
        assert!(slip_won.is_finalized());

        let slip_lost = Slip {
            status: SlipStatus::Lost,
            ..slip_pending
        };
        assert!(slip_lost.is_finalized());

        let slip_cancelled = Slip {
            status: SlipStatus::Cancelled,
            ..slip_pending
        };
        assert!(slip_cancelled.is_finalized());
    }

    #[test]
    fn test_is_leg_bought_settled_won() {
        let slip = Slip {
            owner: Pubkey::default(),
            slip_id: 1,
            epoch_id: 0,
            num_legs: 4,
            leg_market_ids: [0u64; MAX_SLIP_LEGS],
            leg_outcome_ids: [0u8; MAX_SLIP_LEGS],
            legs_bought_mask: 0b0101, // legs 0 and 2
            legs_settled_mask: 0b0101,
            legs_won_mask: 0b0100, // only leg 2
            total_stake: 1000,
            total_cost: 0,
            potential_payout: 0,
            locked_amount: 0,
            status: SlipStatus::Active,
            created_at: 0,
            cancel_deadline: 0,
            bump: 1,
            claimed: false,
        };

        assert!(slip.is_leg_bought(0));
        assert!(!slip.is_leg_bought(1));
        assert!(slip.is_leg_bought(2));
        assert!(!slip.is_leg_bought(3));

        assert!(slip.is_leg_settled(0));
        assert!(!slip.is_leg_settled(1));

        assert!(!slip.is_leg_won(0));
        assert!(slip.is_leg_won(2));

        // Out of range
        assert!(!slip.is_leg_bought(10));
        assert!(!slip.is_leg_settled(10));
        assert!(!slip.is_leg_won(10));
    }
}
