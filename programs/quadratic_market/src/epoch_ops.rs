use crate::constants::{seeds, MIN_FIRST_LIQUIDITY, SCALE};
use crate::errors::QuadraticMarketError;
use crate::state::{Epoch, EpochLpPosition, EpochVault, GlobalConfig};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Token, TokenAccount};

// ─── Init Epoch ────────────────────────────────────────────────
// Creates the on-chain Epoch account for `global_config.current_epoch`.
// Must be called by admin/operator before any markets can be created in
// the new epoch. Idempotent: if the account already exists the handler
// is a no-op (init_if_needed).

#[derive(Accounts)]
pub struct InitEpoch<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        init_if_needed,
        payer = authority,
        space = Epoch::LEN,
        seeds = [seeds::EPOCH, global_config.current_epoch.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch: Account<'info, Epoch>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn init_epoch_handler(ctx: Context<InitEpoch>) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let epoch = &mut ctx.accounts.epoch;

    require!(
        config.is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );

    // Only initialise if this is a fresh account (bump == 0 means just created)
    if epoch.bump == 0 {
        let now = Clock::get()?.unix_timestamp;
        let epoch_duration = config.epoch_duration_seconds;
        let epoch_start = if epoch_duration > 0 {
            (now / epoch_duration) * epoch_duration
        } else {
            now
        };

        epoch.epoch_id = config.current_epoch;
        epoch.start_time = epoch_start;
        epoch.end_time = if epoch_duration > 0 {
            epoch_start + epoch_duration
        } else {
            i64::MAX
        };
        epoch.total_liquidity_added = 0;
        epoch.total_liquidity_removed = 0;
        epoch.num_markets = 0;
        epoch.num_settled_markets = 0;
        epoch.all_markets_settled = true;
        epoch.withdrawals_enabled = true;
        epoch.lp_shares_at_close = config.total_lp_supply;
        epoch.bump = *ctx.bumps.get("epoch").unwrap();

        if epoch_duration > 0 {
            config.next_epoch_start = epoch_start + epoch_duration;
        }
    }

    Ok(())
}

// ─── Pause Epoch ───────────────────────────────────────────────
// Admin-only. Blocks new deposits, withdrawals, and market creation.
// Used to freeze activity between epochs while markets settle.
// Existing bets are unaffected; users can still claim payouts.

#[derive(Accounts)]
pub struct PauseEpoch<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    pub admin: Signer<'info>,
}

pub fn pause_epoch_handler(ctx: Context<PauseEpoch>) -> Result<()> {
    let config = &mut ctx.accounts.global_config;

    require!(
        ctx.accounts.admin.key() == config.admin,
        QuadraticMarketError::Unauthorized
    );

    config.epoch_paused = true;

    Ok(())
}

// ─── Unpause Epoch ─────────────────────────────────────────────
// Admin-only. Re-enables deposits, withdrawals, and market creation.

#[derive(Accounts)]
pub struct UnpauseEpoch<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    pub admin: Signer<'info>,
}

pub fn unpause_epoch_handler(ctx: Context<UnpauseEpoch>) -> Result<()> {
    let config = &mut ctx.accounts.global_config;

    require!(
        ctx.accounts.admin.key() == config.admin,
        QuadraticMarketError::Unauthorized
    );

    config.epoch_paused = false;

    Ok(())
}

// ─── Close Epoch ───────────────────────────────────────────────
// Explicitly marks an epoch as closed and enables LP withdrawals.
// Normally withdrawals are auto-enabled when the last market settles.
// This is a manual override for edge cases
// (e.g. all markets voided, epoch has zero markets).

#[derive(Accounts)]
pub struct CloseEpoch<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::EPOCH, epoch.epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    pub authority: Signer<'info>,
}

pub fn close_epoch_handler(ctx: Context<CloseEpoch>) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let epoch = &mut ctx.accounts.epoch;

    require!(
        config.is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );

    require!(
        epoch.num_markets == 0 || epoch.all_markets_settled,
        QuadraticMarketError::EpochNotComplete
    );

    epoch.lp_shares_at_close = config.total_lp_supply;
    epoch.withdrawals_enabled = true;

    Ok(())
}

// ─── Publish Epoch ──────────────────────────────────────────────
// Publishes an epoch with its market list. This is the announcement
// LPs see before choosing to opt-in. No funds moved yet.

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct PublishEpoch<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        init,
        payer = authority,
        space = Epoch::LEN,
        seeds = [seeds::EPOCH, epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch: Account<'info, Epoch>,

    #[account(
        init,
        payer = authority,
        space = EpochVault::LEN,
        seeds = [seeds::EPOCH_VAULT, epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch_vault: Account<'info, EpochVault>,

    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn publish_epoch_handler(
    ctx: Context<PublishEpoch>,
    epoch_id: u64,
    _market_ids: Vec<u64>, // Future: store market list if needed
) -> Result<()> {
    let config = &ctx.accounts.global_config;
    let epoch = &mut ctx.accounts.epoch;
    let vault = &mut ctx.accounts.epoch_vault;

    require!(
        config.is_authorized(&ctx.accounts.authority.key()),
        QuadraticMarketError::Unauthorized
    );

    let now = Clock::get()?.unix_timestamp;
    let epoch_duration = config.epoch_duration_seconds;

    epoch.epoch_id = epoch_id;
    epoch.num_markets = 0; // Will be incremented as markets are added
    epoch.start_time = now;
    epoch.end_time = if epoch_duration > 0 {
        now + epoch_duration
    } else {
        i64::MAX
    };
    epoch.total_liquidity_added = 0;
    epoch.total_liquidity_removed = 0;
    epoch.num_settled_markets = 0;
    epoch.all_markets_settled = false;
    epoch.withdrawals_enabled = false;
    epoch.lp_shares_at_close = 0;
    epoch.bump = *ctx.bumps.get("epoch").unwrap();

    vault.epoch_id = epoch_id;
    vault.total_deposits = 0;
    vault.total_withdrawals = 0;
    vault.total_shares = 0;
    vault.num_lps = 0;
    vault.created_at = now;
    vault.closed_at = 0;
    vault.withdrawals_enabled = false;
    vault.bump = *ctx.bumps.get("epoch_vault").unwrap();

    emit!(EpochPublished {
        epoch_id,
        start_time: now,
        end_time: epoch.end_time,
    });

    Ok(())
}

// ─── Opt-In Epoch Liquidity ─────────────────────────────────────
// LPs deposit into a specific epoch's vault to back its markets.

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct OptInEpochLiquidity<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::EPOCH_VAULT, epoch_id.to_le_bytes().as_ref()],
        bump = epoch_vault.bump,
    )]
    pub epoch_vault: Account<'info, EpochVault>,

    #[account(
        init,
        payer = lp,
        space = EpochLpPosition::LEN,
        seeds = [b"epoch_lp", epoch_id.to_le_bytes().as_ref(), lp.key().as_ref()],
        bump,
    )]
    pub lp_position: Account<'info, EpochLpPosition>,

    #[account(mut)]
    pub lp_base_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub epoch_vault_base_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub lp: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn opt_in_epoch_liquidity_handler(
    ctx: Context<OptInEpochLiquidity>,
    epoch_id: u64,
    amount: u64,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let vault = &mut ctx.accounts.epoch_vault;
    let position = &mut ctx.accounts.lp_position;

    require!(!config.paused, QuadraticMarketError::Paused);
    require!(!config.epoch_paused, QuadraticMarketError::EpochPaused);
    require!(amount > 0, QuadraticMarketError::InvalidAmount);

    // First depositor gets minimum liquidity protection
    let shares_to_mint = if vault.total_shares == 0 || vault.total_deposits == 0 {
        require!(
            amount > MIN_FIRST_LIQUIDITY,
            QuadraticMarketError::AmountTooSmall
        );
        vault.total_shares = MIN_FIRST_LIQUIDITY;
        amount - MIN_FIRST_LIQUIDITY
    } else {
        // Mint shares proportional to deposit
        ((amount as u128) * (vault.total_shares as u128) / vault.total_deposits as u128) as u64
    };

    // Transfer tokens to epoch vault
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.lp_base_ata.to_account_info(),
                to: ctx.accounts.epoch_vault_base_ata.to_account_info(),
                authority: ctx.accounts.lp.to_account_info(),
            },
            &[],
        ),
        amount,
    )?;

    // Update vault
    vault.total_deposits = vault
        .total_deposits
        .checked_add(amount)
        .ok_or(QuadraticMarketError::MathOverflow)?;
    vault.total_shares = vault
        .total_shares
        .checked_add(shares_to_mint)
        .ok_or(QuadraticMarketError::MathOverflow)?;
    vault.num_lps += 1;

    // Create LP position
    position.owner = ctx.accounts.lp.key();
    position.epoch_id = epoch_id;
    position.shares = shares_to_mint;
    position.withdrawn = false;
    position.bump = *ctx.bumps.get("lp_position").unwrap();

    emit!(EpochLiquidityOptedIn {
        epoch_id,
        lp: ctx.accounts.lp.key(),
        amount,
        shares_minted: shares_to_mint,
    });

    Ok(())
}

// ─── Withdraw Epoch Liquidity ───────────────────────────────────
// LPs withdraw their pro-rata share after epoch settlement.

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct WithdrawEpochLiquidity<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::EPOCH_VAULT, epoch_id.to_le_bytes().as_ref()],
        bump = epoch_vault.bump,
    )]
    pub epoch_vault: Account<'info, EpochVault>,

    #[account(
        mut,
        seeds = [b"epoch_lp", epoch_id.to_le_bytes().as_ref(), lp.key().as_ref()],
        bump = lp_position.bump,
    )]
    pub lp_position: Account<'info, EpochLpPosition>,

    #[account(mut)]
    pub lp_base_ata: Account<'info, TokenAccount>,

    #[account(mut)]
    pub epoch_vault_base_ata: Account<'info, TokenAccount>,

    /// CHECK: Epoch vault authority PDA for token transfers
    #[account(seeds = [seeds::EPOCH_VAULT, epoch_id.to_le_bytes().as_ref()], bump)]
    pub epoch_vault_authority: SystemAccount<'info>,

    pub lp: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

pub fn withdraw_epoch_liquidity_handler(
    ctx: Context<WithdrawEpochLiquidity>,
    epoch_id: u64,
    shares: u64,
) -> Result<()> {
    let vault = &mut ctx.accounts.epoch_vault;
    let position = &mut ctx.accounts.lp_position;

    require!(
        vault.withdrawals_enabled,
        QuadraticMarketError::EpochWithdrawalsNotEnabled
    );
    require!(
        !position.withdrawn,
        QuadraticMarketError::InsufficientLpShares
    );
    require!(
        position.shares >= shares,
        QuadraticMarketError::InsufficientLpShares
    );

    // Calculate withdrawal amount based on share price
    let share_price = vault.share_price();
    let withdrawal_amount = ((shares as u128) * (share_price as u128) / SCALE as u128) as u64;

    require!(withdrawal_amount > 0, QuadraticMarketError::InvalidAmount);

    // Burn shares
    position.shares = position
        .shares
        .checked_sub(shares)
        .ok_or(QuadraticMarketError::MathUnderflow)?;

    if position.shares == 0 {
        position.withdrawn = true;
    }

    // Update vault
    vault.total_shares = vault
        .total_shares
        .checked_sub(shares)
        .ok_or(QuadraticMarketError::MathUnderflow)?;
    vault.total_withdrawals = vault
        .total_withdrawals
        .checked_add(withdrawal_amount)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Transfer tokens to LP
    let epoch_id_bytes = epoch_id.to_le_bytes();
    let vault_seeds: &[&[&[u8]]] = &[&[seeds::EPOCH_VAULT, epoch_id_bytes.as_ref(), &[vault.bump]]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.epoch_vault_base_ata.to_account_info(),
                to: ctx.accounts.lp_base_ata.to_account_info(),
                authority: ctx.accounts.epoch_vault_authority.to_account_info(),
            },
            vault_seeds,
        ),
        withdrawal_amount,
    )?;

    emit!(EpochLiquidityWithdrawn {
        epoch_id,
        lp: ctx.accounts.lp.key(),
        shares_burned: shares,
        amount_withdrawn: withdrawal_amount,
    });

    Ok(())
}

// ─── Enable Epoch Vault Withdrawals ─────────────────────────────
// Called when epoch is fully settled to enable LP withdrawals.

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct EnableEpochWithdrawals<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::EPOCH_VAULT, epoch_id.to_le_bytes().as_ref()],
        bump = epoch_vault.bump,
    )]
    pub epoch_vault: Account<'info, EpochVault>,

    #[account(
        mut,
        seeds = [seeds::EPOCH, epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,
}

pub fn enable_epoch_withdrawals_handler(
    ctx: Context<EnableEpochWithdrawals>,
    epoch_id: u64,
) -> Result<()> {
    let epoch = &ctx.accounts.epoch;
    let vault = &mut ctx.accounts.epoch_vault;

    require!(
        epoch.all_markets_settled,
        QuadraticMarketError::EpochNotComplete
    );

    vault.withdrawals_enabled = true;

    emit!(EpochWithdrawalsEnabled { epoch_id });

    Ok(())
}

// ─── Events ────────────────────────────────────────────────────

#[event]
pub struct EpochPublished {
    pub epoch_id: u64,
    pub start_time: i64,
    pub end_time: i64,
}

#[event]
pub struct EpochLiquidityOptedIn {
    pub epoch_id: u64,
    pub lp: Pubkey,
    pub amount: u64,
    pub shares_minted: u64,
}

#[event]
pub struct EpochLiquidityWithdrawn {
    pub epoch_id: u64,
    pub lp: Pubkey,
    pub shares_burned: u64,
    pub amount_withdrawn: u64,
}

#[event]
pub struct EpochWithdrawalsEnabled {
    pub epoch_id: u64,
}
