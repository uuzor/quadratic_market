use crate::constants::{seeds, MAX_OPERATORS};
use crate::errors::QuadraticMarketError;
use crate::state::GlobalConfig;
use anchor_lang::prelude::*;

// ─── Transfer Admin ────────────────────────────────────────────

#[derive(Accounts)]
pub struct TransferAdmin<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

pub fn transfer_admin_handler(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );
    ctx.accounts.global_config.admin = new_admin;
    Ok(())
}

// ─── Pause / Unpause ───────────────────────────────────────────

#[derive(Accounts)]
pub struct Pause<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

pub fn pause_handler(ctx: Context<Pause>) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );
    ctx.accounts.global_config.paused = true;
    Ok(())
}

#[derive(Accounts)]
pub struct Unpause<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

pub fn unpause_handler(ctx: Context<Unpause>) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );
    ctx.accounts.global_config.paused = false;
    Ok(())
}

// ─── Update Config ─────────────────────────────────────────────

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

pub fn update_config_handler(
    ctx: Context<UpdateConfig>,
    max_market_exposure: Option<u64>,
    challenge_window_seconds: Option<i64>,
    settlement_deadline_seconds: Option<i64>,
    epoch_duration_seconds: Option<i64>,
    withdrawal_cooldown_seconds: Option<i64>,
    max_single_bet: Option<u64>,
    min_odds_bps: Option<u64>,
    max_odds_bps: Option<u64>,
    house_fee_bps: Option<u64>,
    oracle_pubkey: Option<[u8; 32]>,
) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );

    // Bounds guards — prevent misconfiguration that would break protocol invariants:
    //   challenge_window = 0  → oracle can propose + finalize atomically (no review)
    //   epoch_duration = 0    → division by zero in compute_activation_time
    //   house_fee_bps ≥ 10_000  → total charge ≥ 2× cost, effectively stealing from users
    //   settlement_deadline = 0 → any market can be immediately voided
    if let Some(v) = challenge_window_seconds {
        require!(v >= 60, QuadraticMarketError::InvalidAmount); // minimum 1 minute
    }
    if let Some(v) = settlement_deadline_seconds {
        require!(v >= 60, QuadraticMarketError::InvalidAmount); // minimum 1 minute
    }
    if let Some(v) = epoch_duration_seconds {
        require!(v >= 60, QuadraticMarketError::InvalidAmount); // minimum 1 minute, prevents div-by-zero
    }
    if let Some(v) = house_fee_bps {
        require!(v < 10_000, QuadraticMarketError::InvalidAmount); // must be < 100%
    }
    if let Some(v) = min_odds_bps {
        require!(v >= 100, QuadraticMarketError::InvalidAmount); // minimum 1% (1.01x)
    }
    if let Some(v) = max_odds_bps {
        require!(v <= 1_000_000, QuadraticMarketError::InvalidAmount); // maximum 100x
    }

    let config = &mut ctx.accounts.global_config;
    if let Some(v) = max_market_exposure {
        config.max_market_exposure = v;
    }
    if let Some(v) = challenge_window_seconds {
        config.challenge_window_seconds = v;
    }
    if let Some(v) = settlement_deadline_seconds {
        config.settlement_deadline_seconds = v;
    }
    if let Some(v) = epoch_duration_seconds {
        config.epoch_duration_seconds = v;
    }
    if let Some(v) = withdrawal_cooldown_seconds {
        config.withdrawal_cooldown_seconds = v;
    }
    if let Some(v) = max_single_bet {
        config.max_single_bet = v;
    }
    if let Some(v) = min_odds_bps {
        config.min_odds_bps = v;
    }
    if let Some(v) = max_odds_bps {
        config.max_odds_bps = v;
    }
    if let Some(v) = house_fee_bps {
        config.house_fee_bps = v;
    }
    if let Some(v) = oracle_pubkey {
        config.oracle_pubkey = v;
    }
    Ok(())
}

// ─── Operator Management ───────────────────────────────────────

#[derive(Accounts)]
pub struct AddOperator<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

pub fn add_operator_handler(ctx: Context<AddOperator>, operator: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );
    let config = &mut ctx.accounts.global_config;
    require!(
        (config.num_operators as usize) < MAX_OPERATORS,
        QuadraticMarketError::OperatorListFull
    );
    // Idempotent — don't add duplicates
    let already_exists = config.operators[..config.num_operators as usize]
        .iter()
        .any(|op| op == &operator);
    if !already_exists {
        let idx = config.num_operators as usize;
        config.operators[idx] = operator;
        config.num_operators += 1;
    }
    Ok(())
}

#[derive(Accounts)]
pub struct RemoveOperator<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

pub fn remove_operator_handler(ctx: Context<RemoveOperator>, operator: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin.key() == ctx.accounts.global_config.admin,
        QuadraticMarketError::Unauthorized
    );
    let config = &mut ctx.accounts.global_config;
    let n = config.num_operators as usize;
    let pos = config.operators[..n]
        .iter()
        .position(|op| op == &operator)
        .ok_or(QuadraticMarketError::OperatorNotFound)?;
    // Swap-remove to keep the array compact
    config.operators[pos] = config.operators[n - 1];
    config.operators[n - 1] = Pubkey::default();
    config.num_operators -= 1;
    Ok(())
}
