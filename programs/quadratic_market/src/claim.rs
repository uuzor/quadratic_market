use crate::constants::seeds;
use crate::errors::QuadraticMarketError;
use crate::slip::Slip as BetSlip;
use crate::state::{GlobalConfig, Market, MarketStatus};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// ─── Claim Payout ──────────────────────────────────────────────

#[derive(Accounts)]
#[instruction(market_id: u64)]
pub struct ClaimPayout<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Box<Account<'info, Market>>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    #[account(
        mut,
        associated_token::mint = outcome_mint,
        associated_token::authority = claimer,
    )]
    pub claimer_outcome_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = claimer,
    )]
    pub claimer_base_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(
        constraint = outcome_mint.key() == market.outcome_mints[market.winning_outcome as usize] @ QuadraticMarketError::WrongOutcomeToken,
    )]
    pub outcome_mint: Account<'info, Mint>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub claimer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

pub fn claim_payout_handler(ctx: Context<ClaimPayout>, _market_id: u64) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let market = &ctx.accounts.market;

    require!(
        market.status == MarketStatus::Settled,
        QuadraticMarketError::MarketNotSettled
    );

    let amount = ctx.accounts.claimer_outcome_ata.amount;
    require!(amount > 0, QuadraticMarketError::NoWinningPositions);

    // Burn winning outcome tokens
    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Burn {
                mint: ctx.accounts.outcome_mint.to_account_info(),
                from: ctx.accounts.claimer_outcome_ata.to_account_info(),
                authority: ctx.accounts.claimer.to_account_info(),
            },
        ),
        amount,
    )?;

    // Pay 1 base token per outcome token (1:1 redemption)
    let treasury_seeds = &[seeds::TREASURY, &[config.treasury_bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.treasury_base_ata.to_account_info(),
                to: ctx.accounts.claimer_base_ata.to_account_info(),
                authority: ctx.accounts.treasury.to_account_info(),
            },
            &[treasury_seeds],
        ),
        amount,
    )?;

    config.locked_payouts = config.locked_payouts.saturating_sub(amount);

    Ok(())
}

// ─── Claim Paused Bet ──────────────────────────────────────────
// When the protocol is paused (global_config.paused == true), users can
// reclaim their original stake from an unclaimed BetSlip. This prevents
// funds from being permanently locked if the protocol is emergency-paused.
// The slip's locked_amount is returned 1:1 and the slip is closed.

#[derive(Accounts)]
#[instruction(slip_id: u64)]
pub struct ClaimPausedBet<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::SLIP, slip_id.to_le_bytes().as_ref()],
        bump = bet_slip.bump,
        constraint = bet_slip.owner == claimer.key() @ QuadraticMarketError::Unauthorized,
        close = claimer,
    )]
    pub bet_slip: Account<'info, BetSlip>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = claimer,
    )]
    pub claimer_base_ata: Account<'info, TokenAccount>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    pub claimer: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn claim_paused_bet_handler(ctx: Context<ClaimPausedBet>, _slip_id: u64) -> Result<()> {
    let config = &mut ctx.accounts.global_config;

    // Only available when the protocol is paused
    require!(config.paused, QuadraticMarketError::NotPaused);

    let slip = &ctx.accounts.bet_slip;
    require!(!slip.claimed, QuadraticMarketError::SlipAlreadyClaimed);

    let refund = slip.total_stake;
    require!(refund > 0, QuadraticMarketError::InvalidAmount);

    // Release the locked payout that was reserved for this slip
    config.locked_payouts = config.locked_payouts.saturating_sub(slip.locked_amount);

    let treasury_seeds = &[seeds::TREASURY, &[config.treasury_bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.treasury_base_ata.to_account_info(),
                to: ctx.accounts.claimer_base_ata.to_account_info(),
                authority: ctx.accounts.treasury.to_account_info(),
            },
            &[treasury_seeds],
        ),
        refund,
    )?;

    // Mark as claimed before closing
    let slip = &mut ctx.accounts.bet_slip;
    slip.claimed = true;

    // Slip account is closed via `close = claimer` — rent returned to user.
    Ok(())
}

// ─── Close Market ──────────────────────────────────────────────
// Reclaims rent once a market is fully settled or voided.

#[derive(Accounts)]
#[instruction(market_id: u64)]
pub struct CloseMarket<'info> {
    #[account(
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()],
        bump = market.bump,
        constraint = market.status == MarketStatus::Settled
            || market.status == MarketStatus::Voided
            @ QuadraticMarketError::InvalidMarketStatus,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

pub fn close_market_handler(ctx: Context<CloseMarket>, _market_id: u64) -> Result<()> {
    // Only admin or the market creator can close
    require!(
        ctx.accounts.authority.key() == ctx.accounts.market.creator
            || ctx.accounts.authority.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );

    // Zero the discriminator before draining lamports. The previous manual
    // lamport drain left the 8-byte discriminator intact, allowing the PDA to
    // be re-initialized and old stale data to be read by claim/slip logic.
    let market_account = ctx.accounts.market.to_account_info();
    let mut data = market_account.try_borrow_mut_data()?;
    data[0..8].fill(0);
    drop(data);

    // Return rent to authority
    let lamports = market_account.lamports();
    **market_account.try_borrow_mut_lamports()? = 0;
    **ctx
        .accounts
        .authority
        .to_account_info()
        .try_borrow_mut_lamports()? += lamports;

    Ok(())
}
