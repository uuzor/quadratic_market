use crate::constants::seeds;
use crate::errors::QuadraticMarketError;
use crate::state::{GlobalConfig, Market, MarketStatus};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// ─── Buy Shares (Fixed Odds) ─────────────────────────────────────
// Simple fixed odds trading - user pays stake, gets outcome tokens 1:1
// Payout is determined by the market's fixed odds at settlement time

#[derive(Accounts)]
#[instruction(outcome_id: u8)]
pub struct BuyShares<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(mut, seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()], bump = market.bump)]
    pub market: Box<Account<'info, Market>>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = buyer)]
    pub buyer_base_ata: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = treasury)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = outcome_mint, associated_token::authority = buyer)]
    pub buyer_outcome_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = outcome_mint.key() == market.outcome_mints[outcome_id as usize] @ QuadraticMarketError::WrongOutcomeToken,
    )]
    pub outcome_mint: Account<'info, Mint>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub buyer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn buy_shares_handler(ctx: Context<BuyShares>, outcome_id: u8, stake: u64) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);

    let market = &mut ctx.accounts.market;
    require!(
        market.status == MarketStatus::Open,
        QuadraticMarketError::MarketNotOpen
    );
    require!(
        (outcome_id as usize) < market.num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );

    // Betting closes when match starts
    let now = Clock::get()?.unix_timestamp;
    require!(now < market.start_time, QuadraticMarketError::MarketExpired);

    // Max single-bet guard
    require!(
        stake <= config.max_single_bet,
        QuadraticMarketError::BetTooLarge
    );
    require!(stake > 0, QuadraticMarketError::InvalidAmount);

    // Get fixed odds for this outcome (in basis points)
    let odds = market.odds[outcome_id as usize];
    require!(
        odds >= config.min_odds_bps,
        QuadraticMarketError::InvalidAmount
    );
    require!(
        odds <= config.max_odds_bps,
        QuadraticMarketError::InvalidAmount
    );

    // Apply house fee first
    let fee = stake
        .checked_mul(config.house_fee_bps)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;
    let net_stake = stake
        .checked_sub(fee)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Calculate payout on net_stake (after fee)
    let potential_payout = net_stake
        .checked_mul(odds)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;

    // Check treasury has enough liquidity for potential payout
    let free_liquidity = config.free_liquidity(ctx.accounts.treasury_base_ata.amount);
    require!(
        free_liquidity >= potential_payout,
        QuadraticMarketError::InsufficientLiquidity
    );

    // Transfer stake to treasury
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.buyer_base_ata.to_account_info(),
                to: ctx.accounts.treasury_base_ata.to_account_info(),
                authority: ctx.accounts.buyer.to_account_info(),
            },
        ),
        stake,
    )?;

    // Mint outcome tokens equal to potential_payout (1:1 backing)
    let market_id_bytes = market.market_id.to_le_bytes();
    let market_seeds = &[seeds::MARKET, market_id_bytes.as_ref(), &[market.bump]];
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::MintTo {
                mint: ctx.accounts.outcome_mint.to_account_info(),
                to: ctx.accounts.buyer_outcome_ata.to_account_info(),
                authority: market.to_account_info(),
            },
            &[market_seeds],
        ),
        potential_payout,
    )?;

    // Update market exposure (potential liability)
    market.exposure = market
        .exposure
        .checked_add(potential_payout)
        .ok_or(QuadraticMarketError::MathOverflow)?;
    require!(
        market.exposure <= config.max_market_exposure,
        QuadraticMarketError::MaxExposureReached
    );

    // Lock the potential payout
    config.locked_payouts = config
        .locked_payouts
        .checked_add(potential_payout)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    Ok(())
}

// ─── Sell Shares (Fixed Odds) ─────────────────────────────────────
// User sells their outcome tokens back - payout based on current fixed odds

#[derive(Accounts)]
#[instruction(outcome_id: u8)]
pub struct SellShares<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(mut, seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()], bump = market.bump)]
    pub market: Box<Account<'info, Market>>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    #[account(mut, associated_token::mint = outcome_mint, associated_token::authority = seller)]
    pub seller_outcome_ata: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = seller)]
    pub seller_base_ata: Account<'info, TokenAccount>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = treasury)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = outcome_mint.key() == market.outcome_mints[outcome_id as usize] @ QuadraticMarketError::WrongOutcomeToken,
    )]
    pub outcome_mint: Account<'info, Mint>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub seller: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub fn sell_shares_handler(
    ctx: Context<SellShares>,
    outcome_id: u8,
    num_shares: u64,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);

    let market = &mut ctx.accounts.market;
    require!(
        market.status == MarketStatus::Open,
        QuadraticMarketError::MarketNotOpen
    );
    require!(
        (outcome_id as usize) < market.num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );
    require!(
        ctx.accounts.seller_outcome_ata.amount >= num_shares,
        QuadraticMarketError::InsufficientShares
    );

    // Selling closes when match starts
    let now = Clock::get()?.unix_timestamp;
    require!(now < market.start_time, QuadraticMarketError::MarketExpired);

    // Calculate payout based on current fixed odds
    let odds = market.odds[outcome_id as usize];
    let payout = num_shares
        .checked_mul(odds)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;

    // Check treasury has enough liquidity
    let free_liquidity = config.free_liquidity(ctx.accounts.treasury_base_ata.amount);
    require!(
        free_liquidity >= payout,
        QuadraticMarketError::InsufficientLiquidity
    );

    // Burn outcome tokens
    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Burn {
                mint: ctx.accounts.outcome_mint.to_account_info(),
                from: ctx.accounts.seller_outcome_ata.to_account_info(),
                authority: ctx.accounts.seller.to_account_info(),
            },
        ),
        num_shares,
    )?;

    // Transfer payout from treasury
    let treasury_seeds = &[seeds::TREASURY, &[config.treasury_bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.treasury_base_ata.to_account_info(),
                to: ctx.accounts.seller_base_ata.to_account_info(),
                authority: ctx.accounts.treasury.to_account_info(),
            },
            &[treasury_seeds],
        ),
        payout,
    )?;

    // Reduce market exposure and locked payouts
    let exposure_reduction = num_shares
        .checked_mul(odds)
        .ok_or(QuadraticMarketError::MathOverflow)?
        / 10000;
    market.exposure = market.exposure.saturating_sub(exposure_reduction);
    config.locked_payouts = config.locked_payouts.saturating_sub(payout);

    Ok(())
}
