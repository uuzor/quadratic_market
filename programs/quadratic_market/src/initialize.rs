use crate::constants::{
    seeds, DEFAULT_CHALLENGE_WINDOW, DEFAULT_EPOCH_DURATION_SECONDS, DEFAULT_HOUSE_FEE_BPS,
    DEFAULT_MAX_ODDS_BPS, DEFAULT_MAX_SINGLE_BET, DEFAULT_MIN_ODDS_BPS,
    DEFAULT_SETTLEMENT_DEADLINE, DEFAULT_WITHDRAWAL_COOLDOWN_SECONDS, MAX_OPERATORS,
    MIN_FIRST_LIQUIDITY,
};
use crate::state::GlobalConfig;
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = GlobalConfig::LEN,
        seeds = [seeds::GLOBAL_CONFIG],
        bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        init,
        payer = admin,
        seeds = [seeds::LP_MINT],
        bump,
        mint::decimals = 6,
        mint::authority = global_config,
    )]
    pub lp_mint: Account<'info, Mint>,

    /// CHECK: Treasury PDA — owns token accounts, no data needed
    #[account(
        seeds = [seeds::TREASURY],
        bump,
    )]
    pub treasury: SystemAccount<'info>,

    pub base_mint: Account<'info, Mint>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(
    ctx: Context<Initialize>,
    oracle_pubkey: [u8; 32],
    max_market_exposure: u64,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;

    config.admin = ctx.accounts.admin.key();
    config.paused = false;
    config.oracle_pubkey = oracle_pubkey;
    config.max_market_exposure = max_market_exposure;
    config.locked_payouts = 0;
    config.total_lp_supply = 0;
    config.lp_mint = ctx.accounts.lp_mint.key();
    config.base_mint = ctx.accounts.base_mint.key();
    config.treasury = ctx.accounts.treasury.key();
    config.treasury_bump = *ctx.bumps.get("treasury").unwrap();
    config.next_market_id = 1;
    config.challenge_window_seconds = DEFAULT_CHALLENGE_WINDOW;
    config.settlement_deadline_seconds = DEFAULT_SETTLEMENT_DEADLINE;
    config.min_first_liquidity = MIN_FIRST_LIQUIDITY;
    config.next_slip_id = 1;
    config.current_epoch = 0;
    config.epoch_duration_seconds = DEFAULT_EPOCH_DURATION_SECONDS;
    config.withdrawal_cooldown_seconds = DEFAULT_WITHDRAWAL_COOLDOWN_SECONDS;
    config.max_single_bet = DEFAULT_MAX_SINGLE_BET;
    config.min_odds_bps = DEFAULT_MIN_ODDS_BPS;
    config.max_odds_bps = DEFAULT_MAX_ODDS_BPS;
    config.house_fee_bps = DEFAULT_HOUSE_FEE_BPS;
    config.operators = [Pubkey::default(); MAX_OPERATORS];
    config.num_operators = 0;
    config.bump = *ctx.bumps.get("global_config").unwrap();
    config.next_order_id = 1;
    config.order_collateral_locked = 0;
    // Epoch state: start unpaused, epoch 0, next epoch starts one duration from now
    config.epoch_paused = false;
    config.next_epoch_start = 0; // will be set when first epoch is initialized

    Ok(())
}
