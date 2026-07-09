use crate::constants::{
    seeds, BASE_MINT_DECIMALS, MAX_DESCRIPTION_LEN, MAX_OUTCOMES, MAX_TITLE_LEN, SCALE,
};
use crate::errors::QuadraticMarketError;
use crate::math::lmsr;
use crate::state::{Epoch, GlobalConfig, Market, MarketMode, MarketStatus};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// ─── Create Market (operator/admin only) ───────────────────────

#[derive(Accounts)]
pub struct CreateMarket<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        init,
        payer = authority,
        space = Market::LEN,
        seeds = [seeds::MARKET, global_config.next_market_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub market: Box<Account<'info, Market>>,

    /// Epoch account for the current epoch — must match global_config.current_epoch.
    /// Markets are always created under the active epoch so the epoch can track
    /// how many markets need to settle before LP withdrawals are unlocked.
    #[account(
        mut,
        seeds = [seeds::EPOCH, global_config.current_epoch.to_le_bytes().as_ref()],
        bump = epoch.bump,
        constraint = epoch.epoch_id == global_config.current_epoch @ QuadraticMarketError::EpochAccountMismatch,
    )]
    pub epoch: Account<'info, Epoch>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn create_market_handler(
    ctx: Context<CreateMarket>,
    start_time: i64,
    num_outcomes: u8,
    title: String,
    description: String,
    category: u8,
    lmsr_b_override: Option<u64>,
    initial_q_values: Option<Vec<u64>>,
    market_mode: MarketMode,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);
    // Epoch must not be paused — no new markets during inter-epoch gap
    require!(!config.epoch_paused, QuadraticMarketError::EpochPaused);
    require!(
        config.is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );

    require!(
        num_outcomes >= 2 && (num_outcomes as usize) <= MAX_OUTCOMES,
        QuadraticMarketError::InvalidNumOutcomes
    );

    let now = Clock::get()?.unix_timestamp;
    require!(start_time > now, QuadraticMarketError::MarketAlreadyStarted);

    require!(
        title.len() <= MAX_TITLE_LEN && !title.is_empty(),
        QuadraticMarketError::InvalidAmount
    );
    require!(
        description.len() <= MAX_DESCRIPTION_LEN,
        QuadraticMarketError::InvalidAmount
    );

    let lmsr_b = lmsr_b_override.unwrap_or(config.lmsr_default_b);

    // Validate initial_q_values if provided — reject absurd prices that could
    // hand free arb to the first bettor. Reject any outcome whose implied LMSR
    // price falls outside [1%, 99%]. This guards against bot/API bugs seeding
    // markets at near-certainty prices (0% or 100%).
    if let Some(ref q_vals) = initial_q_values {
        require!(
            q_vals.len() == num_outcomes as usize,
            QuadraticMarketError::InvalidOutcomeId
        );

        // Convert to array for lmsr_price
        let mut q_array = [0u64; MAX_OUTCOMES];
        for i in 0..num_outcomes as usize {
            q_array[i] = q_vals[i];
        }

        // Bounds: 1% (0.01 * SCALE) to 99% (0.99 * SCALE)
        let min_price = SCALE / 100; // 1%
        let max_price = (SCALE * 99) / 100; // 99%

        for outcome_id in 0..num_outcomes {
            let price = lmsr::lmsr_price(&q_array, num_outcomes, outcome_id, lmsr_b)?;
            require!(
                price >= min_price && price <= max_price,
                QuadraticMarketError::InvalidAmount
            );
        }
    }

    let current_epoch_id = config.current_epoch;

    let market = &mut ctx.accounts.market;
    market.market_id = config.next_market_id;
    market.creator = ctx.accounts.authority.key();
    market.start_time = start_time;
    market.status = MarketStatus::PreOpen;
    market.num_outcomes = num_outcomes;

    let mut q_values = [0u64; MAX_OUTCOMES];
    if let Some(q_vals) = initial_q_values {
        for i in 0..num_outcomes as usize {
            q_values[i] = q_vals[i];
        }
    }
    market.q_values = q_values;
    market.exposure = 0;
    market.settlement_time = 0;
    market.winning_outcome = 0;
    market.outcome_mints = [Pubkey::default(); MAX_OUTCOMES];
    market.lmsr_b = lmsr_b;
    market.title = title;
    market.description = description;
    market.category = category;
    market.bump = ctx.bumps.market;
    market.group_id = None;
    market.group_market_index = 0;
    market.market_mode = market_mode;
    // Bind market to the current epoch
    market.epoch_id = current_epoch_id;
    market.settled_in_epoch = false;
    market.backing = 0;
    market.seed_fee_pool = 0;
    market.locked_payout = 0;

    config.next_market_id = config
        .next_market_id
        .checked_add(1)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Register this market in the epoch so settlement tracking is accurate
    let epoch = &mut ctx.accounts.epoch;
    epoch.num_markets = epoch
        .num_markets
        .checked_add(1)
        .ok_or(QuadraticMarketError::MathOverflow)?;
    epoch.all_markets_settled = false;
    epoch.withdrawals_enabled = false;
    epoch.lp_shares_at_close = 0;

    Ok(())
}

// ─── Init Outcome Mint ─────────────────────────────────────────

#[derive(Accounts)]
#[instruction(market_id: u64, outcome_id: u8)]
pub struct InitOutcomeMint<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    #[account(
        init,
        payer = payer,
        seeds = [seeds::OUTCOME_MINT, market_id.to_le_bytes().as_ref(), outcome_id.to_le_bytes().as_ref()],
        bump,
        mint::decimals = BASE_MINT_DECIMALS,
        mint::authority = market,
    )]
    pub outcome_mint: Account<'info, Mint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn init_outcome_mint_handler(
    ctx: Context<InitOutcomeMint>,
    _market_id: u64,
    outcome_id: u8,
) -> Result<()> {
    let market = &mut ctx.accounts.market;
    require!(
        (outcome_id as usize) < market.num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );
    require!(
        market.outcome_mints[outcome_id as usize] == Pubkey::default(),
        QuadraticMarketError::InvalidAmount
    );
    market.outcome_mints[outcome_id as usize] = ctx.accounts.outcome_mint.key();
    Ok(())
}

// ─── Suspend / Resume Market ───────────────────────────────────

#[derive(Accounts)]
pub struct SuspendMarket<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(mut, seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()], bump = market.bump)]
    pub market: Account<'info, Market>,

    pub authority: Signer<'info>,
}

pub fn suspend_market_handler(ctx: Context<SuspendMarket>) -> Result<()> {
    require!(
        ctx.accounts
            .global_config
            .is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );
    require!(
        ctx.accounts.market.status == MarketStatus::Open,
        QuadraticMarketError::InvalidMarketStatus
    );
    ctx.accounts.market.status = MarketStatus::Suspended;
    Ok(())
}

#[derive(Accounts)]
pub struct ResumeMarket<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(mut, seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()], bump = market.bump)]
    pub market: Account<'info, Market>,

    pub authority: Signer<'info>,
}

pub fn resume_market_handler(ctx: Context<ResumeMarket>) -> Result<()> {
    require!(
        ctx.accounts
            .global_config
            .is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );
    require!(
        ctx.accounts.market.status == MarketStatus::Suspended,
        QuadraticMarketError::InvalidMarketStatus
    );
    // Only resume if match hasn't started yet
    let now = Clock::get()?.unix_timestamp;
    require!(
        now < ctx.accounts.market.start_time,
        QuadraticMarketError::MarketExpired
    );
    ctx.accounts.market.status = MarketStatus::Open;
    Ok(())
}

// ─── Void Market (admin) ───────────────────────────────────────

#[derive(Accounts)]
pub struct VoidMarket<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(mut, seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()], bump = market.bump)]
    pub market: Account<'info, Market>,

    #[account(
        mut,
        seeds = [seeds::EPOCH, market.epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    pub admin: Signer<'info>,
}

pub fn void_market_handler(ctx: Context<VoidMarket>) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );
    require!(
        ctx.accounts.market.status != MarketStatus::Settled
            && ctx.accounts.market.status != MarketStatus::Voided,
        QuadraticMarketError::MarketNotVoidable
    );
    let market = &mut ctx.accounts.market;
    let config = &mut ctx.accounts.global_config;
    let epoch = &mut ctx.accounts.epoch;
    // Release the full outstanding share liability (sum of all q_values), not
    // market.exposure. market.locked_payout tracks the 1:1 redemption
    // obligation; market.exposure is the net-risk delta (num_shares - cost),
    // which is always less. Using exposure would leave the difference frozen.
    let total_locked: u64 = (0..market.num_outcomes as usize)
        .map(|i| market.q_values[i])
        .fold(0u64, |acc, v| acc.saturating_add(v));
    market.locked_payout = market.locked_payout.saturating_sub(total_locked);
    if !market.settled_in_epoch {
        market.settled_in_epoch = true;
        epoch.num_settled_markets = epoch
            .num_settled_markets
            .checked_add(1)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        if epoch.num_markets > 0 && epoch.num_settled_markets >= epoch.num_markets {
            epoch.all_markets_settled = true;
            epoch.lp_shares_at_close = config.total_lp_supply;
            epoch.withdrawals_enabled = true;
        }
    }
    market.status = MarketStatus::Voided;
    Ok(())
}

// ─── Void If Expired (permissionless) ─────────────────────────
// Any caller can trigger auto-void if the oracle never settled the market
// within `settlement_deadline_seconds` of `start_time`.

#[derive(Accounts)]
pub struct VoidIfExpired<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(mut, seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()], bump = market.bump)]
    pub market: Account<'info, Market>,

    #[account(
        mut,
        seeds = [seeds::EPOCH, market.epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,
}

pub fn void_if_expired_handler(ctx: Context<VoidIfExpired>) -> Result<()> {
    let market = &mut ctx.accounts.market;
    let config = &mut ctx.accounts.global_config;
    let epoch = &mut ctx.accounts.epoch;

    // Only Suspended markets can be auto-voided. An Open market is still accepting
    // bets and must be explicitly suspended by an operator before it can be voided.
    // This prevents a race where a market is voided while users are still placing bets.
    require!(
        market.status == MarketStatus::Suspended,
        QuadraticMarketError::MarketNotVoidable
    );

    // Deadline = start_time + settlement_deadline_seconds
    let deadline = market
        .start_time
        .checked_add(config.settlement_deadline_seconds)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    let now = Clock::get()?.unix_timestamp;
    require!(
        now > deadline,
        QuadraticMarketError::SettlementDeadlineNotPassed
    );

    // Same fix as void_market: release sum(q_values), not market.exposure.
    let total_locked: u64 = (0..market.num_outcomes as usize)
        .map(|i| market.q_values[i])
        .fold(0u64, |acc, v| acc.saturating_add(v));
    market.locked_payout = market.locked_payout.saturating_sub(total_locked);
    if !market.settled_in_epoch {
        market.settled_in_epoch = true;
        epoch.num_settled_markets = epoch
            .num_settled_markets
            .checked_add(1)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        if epoch.num_markets > 0 && epoch.num_settled_markets >= epoch.num_markets {
            epoch.all_markets_settled = true;
            epoch.lp_shares_at_close = config.total_lp_supply;
            epoch.withdrawals_enabled = true;
        }
    }
    market.status = MarketStatus::Voided;

    Ok(())
}
