use crate::constants::seeds;
use crate::errors::QuadraticMarketError;
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

    // Must be writable: claim_payout burns the claimer's winning outcome tokens,
    // which decrements this mint's supply (a write to the mint account).
    #[account(
        mut,
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
    let market = &mut ctx.accounts.market;

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
    require!(
        market.backing >= amount,
        QuadraticMarketError::InsufficientMarketBacking
    );
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

    market.backing = market.backing.saturating_sub(amount);
    market.locked_payout = market.locked_payout.saturating_sub(amount);

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
