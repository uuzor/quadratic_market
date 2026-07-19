use crate::constants::{
    seeds, BASE_MINT_DECIMALS, MAX_DESCRIPTION_LEN, MAX_OUTCOMES, MAX_TITLE_LEN,
};
use crate::errors::QuadraticMarketError;
use crate::settlement_with_proof::{validate_txoracle_proof, NDimensionalStrategy, StatValidationInput};
use crate::state::{Epoch, GlobalConfig, Market, MarketStatus, MarketType};
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};

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
    market_type: MarketType,
    initial_odds: Vec<u64>, // Fixed odds in basis points for each outcome
    txline_fixture_id: Option<u64>, // Optional TxLINE fixture ID for verifiable settlement
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

    require!(
        initial_odds.len() == num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );

    // Validate odds are within bounds
    for odds in &initial_odds {
        require!(
            *odds >= config.min_odds_bps,
            QuadraticMarketError::InvalidAmount
        );
        require!(
            *odds <= config.max_odds_bps,
            QuadraticMarketError::InvalidAmount
        );
    }

    let current_epoch_id = config.current_epoch;

    let market = &mut ctx.accounts.market;
    market.market_id = config.next_market_id;
    market.creator = ctx.accounts.authority.key();
    market.start_time = start_time;
    market.status = MarketStatus::Open;
    market.num_outcomes = num_outcomes;

    // Initialize odds array with provided values
    let mut odds = [0u64; MAX_OUTCOMES];
    for i in 0..num_outcomes as usize {
        odds[i] = initial_odds[i];
    }
    market.odds = odds;
    market.exposure = 0;
    market.settlement_time = 0;
    market.winning_outcome = 0;
    market.outcome_mints = [Pubkey::default(); MAX_OUTCOMES];
    market.title = title;
    market.description = description;
    market.market_type = market_type;
    market.category = category;
    market.bump = *ctx.bumps.get("market").unwrap();
    market.group_id = None;
    // Bind market to the current epoch
    market.epoch_id = current_epoch_id;
    market.settled_in_epoch = false;
    // TxLINE fixture reference for proof-based settlement
    market.txline_fixture_id = txline_fixture_id;
    market.txline_proof_verified = false;

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

    // Release the exposure locked for this market
    config.locked_payouts = config.locked_payouts.saturating_sub(market.exposure);

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

    // Release the exposure locked for this market
    config.locked_payouts = config.locked_payouts.saturating_sub(market.exposure);

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

// ─── Update Market Odds (operator/admin only) ─────────────────
// Can update odds until match starts. Odds in basis points.

#[derive(Accounts)]
#[instruction(market_id: u64)]
pub struct UpdateMarketOdds<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(market_id: u64)]
pub struct UpdateMarketOddsWithProof<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    /// CHECK: Read-only CPI account required by Txoracle.
    pub daily_scores_merkle_roots: AccountInfo<'info>,

    /// CHECK: Txoracle program account.
    pub txoracle_program: AccountInfo<'info>,

    pub authority: Signer<'info>,
}

pub fn update_market_odds_handler(
    ctx: Context<UpdateMarketOdds>,
    _market_id: u64,
    new_odds: Vec<u64>,
) -> Result<()> {
    let config = &ctx.accounts.global_config;
    let market = &mut ctx.accounts.market;

    require!(
        config.is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );

    require!(
        market.status == MarketStatus::Open,
        QuadraticMarketError::InvalidMarketStatus
    );

    require!(
        new_odds.len() == market.num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );

    // Can only update odds until match starts
    let now = Clock::get()?.unix_timestamp;
    require!(now < market.start_time, QuadraticMarketError::MarketExpired);

    // Validate and update odds
    for i in 0..market.num_outcomes as usize {
        require!(
            new_odds[i] >= config.min_odds_bps,
            QuadraticMarketError::InvalidAmount
        );
        require!(
            new_odds[i] <= config.max_odds_bps,
            QuadraticMarketError::InvalidAmount
        );
        market.odds[i] = new_odds[i];
    }

    Ok(())
}

pub fn update_market_odds_with_proof_handler(
    ctx: Context<UpdateMarketOddsWithProof>,
    _market_id: u64,
    new_odds: Vec<u64>,
    validation_input: StatValidationInput,
    strategy: NDimensionalStrategy,
) -> Result<()> {
    let config = &ctx.accounts.global_config;
    let market = &mut ctx.accounts.market;

    require!(
        config.is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );

    require!(
        market.status == MarketStatus::Open,
        QuadraticMarketError::InvalidMarketStatus
    );

    require!(
        new_odds.len() == market.num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );

    let now = Clock::get()?.unix_timestamp;
    require!(now < market.start_time, QuadraticMarketError::MarketExpired);

    if let Some(fixture_id) = market.txline_fixture_id {
        require!(
            validation_input.fixture_summary.fixture_id == fixture_id as i64,
            QuadraticMarketError::InvalidTxlineFixtureId
        );
    }

    validate_txoracle_proof(
        &ctx.accounts.txoracle_program,
        &ctx.accounts.daily_scores_merkle_roots,
        validation_input,
        strategy,
    )?;

    for i in 0..market.num_outcomes as usize {
        require!(
            new_odds[i] >= config.min_odds_bps,
            QuadraticMarketError::InvalidAmount
        );
        require!(
            new_odds[i] <= config.max_odds_bps,
            QuadraticMarketError::InvalidAmount
        );
        market.odds[i] = new_odds[i];
    }

    Ok(())
}
