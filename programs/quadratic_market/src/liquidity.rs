use crate::constants::{seeds, SCALE};
use crate::errors::QuadraticMarketError;
use crate::state::{Epoch, GlobalConfig, PendingLiquidity, WithdrawalRequest};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// ─── Helper: compute fixed activation time ─────────────────────
fn compute_activation_time(now: i64, epoch_duration: i64) -> i64 {
    if epoch_duration <= 0 {
        return now;
    }
    let epoch_start = (now / epoch_duration) * epoch_duration;
    epoch_start + 2 * epoch_duration
}

// ─── Add Liquidity ─────────────────────────────────────────────
// pending_liquidity is initialised here with on-chain-computed shares and
// activation_time. The old separate init_pending_liquidity instruction
// accepted these as caller-supplied parameters, allowing the epoch lock to
// be bypassed by passing activation_time=0 or inflated shares. Fixed by
// computing both values inside add_liquidity_handler.

#[derive(Accounts)]
pub struct AddLiquidity<'info> {
    // Boxed: GlobalConfig is ~564 bytes; leaving it on the stack pushes try_accounts
    // over the 4096-byte BPF frame limit.
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        constraint = lp_mint.key() == global_config.lp_mint @ QuadraticMarketError::Unauthorized,
    )]
    pub lp_mint: Account<'info, Mint>,

    /// CHECK: Treasury PDA
    #[account(
        seeds = [seeds::TREASURY],
        bump = global_config.treasury_bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_base_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = provider,
    )]
    pub provider_base_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = provider,
    )]
    pub provider_lp_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized
    )]
    pub base_mint: Account<'info, Mint>,

    // Pending liquidity PDA — initialised here so shares and activation_time
    // are set by the program, not the caller.
    #[account(
        init_if_needed,
        payer = provider,
        space = PendingLiquidity::LEN,
        seeds = [seeds::PENDING, provider.key().as_ref()],
        bump,
    )]
    pub pending_liquidity: Account<'info, PendingLiquidity>,

    #[account(mut)]
    pub provider: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn add_liquidity_handler(ctx: Context<AddLiquidity>, amount: u64) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);
    // No deposits while epoch is paused (between epochs, during settlement)
    require!(!config.epoch_paused, QuadraticMarketError::EpochPaused);
    require!(amount > 0, QuadraticMarketError::InvalidAmount);

    let now = Clock::get()?.unix_timestamp;

    let activation_time = compute_activation_time(now, config.epoch_duration_seconds);
    let reserve_balance = ctx.accounts.treasury_base_ata.amount;
    let total_supply = config.total_lp_supply;

    let shares_to_mint = if total_supply == 0 || reserve_balance == 0 {
        require!(
            amount > config.min_first_liquidity,
            QuadraticMarketError::AmountTooSmall
        );
        config.total_lp_supply = config.min_first_liquidity;
        amount - config.min_first_liquidity
    } else {
        ((amount as u128)
            .checked_mul(total_supply as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?)
        .checked_div(reserve_balance as u128)
        .ok_or(QuadraticMarketError::MathOverflow)? as u64
    };

    require!(shares_to_mint > 0, QuadraticMarketError::InvalidAmount);

    // Transfer base tokens
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.provider_base_ata.to_account_info(),
                to: ctx.accounts.treasury_base_ata.to_account_info(),
                authority: ctx.accounts.provider.to_account_info(),
            },
        ),
        amount,
    )?;

    // Mint LP tokens
    let bump = config.bump;
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::MintTo {
                mint: ctx.accounts.lp_mint.to_account_info(),
                to: ctx.accounts.provider_lp_ata.to_account_info(),
                authority: config.to_account_info(),
            },
            &[&[seeds::GLOBAL_CONFIG, &[bump]]],
        ),
        shares_to_mint,
    )?;

    config.total_lp_supply = config
        .total_lp_supply
        .checked_add(shares_to_mint)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    // Write pending_liquidity with on-chain-computed values.
    // activation_time and shares_to_mint are derived here — the caller cannot
    // supply fake values to bypass the epoch lock.
    let pending = &mut ctx.accounts.pending_liquidity;
    if pending.lp == Pubkey::default() {
        pending.lp = ctx.accounts.provider.key();
        pending.shares = shares_to_mint;
        pending.activation_time = activation_time;
        pending.amount_deposited = amount;
        pending.bump = *ctx.bumps.get("pending_liquidity").unwrap();
    } else {
        pending.shares = pending
            .shares
            .checked_add(shares_to_mint)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        pending.amount_deposited = pending
            .amount_deposited
            .checked_add(amount)
            .ok_or(QuadraticMarketError::MathOverflow)?;
        // Keep the earliest activation_time (most conservative lock)
        if activation_time > pending.activation_time {
            pending.activation_time = activation_time;
        }
    }

    Ok(())
}

// ─── Request Withdrawal ────────────────────────────────────────

#[derive(Accounts)]
pub struct RequestWithdraw<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        constraint = lp_mint.key() == global_config.lp_mint @ QuadraticMarketError::Unauthorized,
    )]
    pub lp_mint: Account<'info, Mint>,

    /// CHECK: Treasury PDA
    #[account(
        seeds = [seeds::TREASURY],
        bump = global_config.treasury_bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    // Boxed: three TokenAccounts (3 × 165 bytes) push the frame 8 bytes over the
    // 4096-byte BPF limit. Boxing all three gives ~487 bytes of headroom.
    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_base_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_lp_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = lp,
    )]
    pub lp_lp_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Pending liquidity PDA
    #[account(
        seeds = [seeds::PENDING, lp.key().as_ref()],
        bump,
    )]
    pub pending_liquidity: UncheckedAccount<'info>,

    #[account(
        init,
        payer = lp,
        space = WithdrawalRequest::LEN,
        seeds = [seeds::WITHDRAWAL, lp.key().as_ref()],
        bump,
    )]
    pub withdrawal_request: Account<'info, WithdrawalRequest>,

    #[account(
        constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized
    )]
    pub base_mint: Account<'info, Mint>,

    /// Epoch account for the current epoch. Withdrawals are only permitted
    /// once all markets in the epoch have settled (withdrawals_enabled == true).
    #[account(
        seeds = [seeds::EPOCH, global_config.current_epoch.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    #[account(mut)]
    pub lp: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn request_withdraw_handler(ctx: Context<RequestWithdraw>, shares: u64) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);
    // Epoch must not be paused and withdrawals must be enabled for this epoch
    require!(!config.epoch_paused, QuadraticMarketError::EpochPaused);
    require!(
        ctx.accounts.epoch.withdrawals_enabled,
        QuadraticMarketError::EpochWithdrawalsNotEnabled
    );
    require!(shares > 0, QuadraticMarketError::InvalidAmount);
    require!(
        ctx.accounts.lp_lp_ata.amount >= shares,
        QuadraticMarketError::InsufficientLpShares
    );

    // Check locked shares
    let pending_locked: u64 = {
        let pending_data = ctx.accounts.pending_liquidity.data.borrow();
        if pending_data.len() >= PendingLiquidity::LEN {
            let mut disc = [0u8; 8];
            disc.copy_from_slice(&pending_data[0..8]);
            if disc != [0u8; 8] {
                let mut lp_bytes = [0u8; 32];
                lp_bytes.copy_from_slice(&pending_data[8..40]);
                let pending_lp = Pubkey::new_from_array(lp_bytes);
                if pending_lp != Pubkey::default() {
                    let mut shares_bytes = [0u8; 8];
                    shares_bytes.copy_from_slice(&pending_data[40..48]);
                    let shares = u64::from_le_bytes(shares_bytes);
                    let mut time_bytes = [0u8; 8];
                    time_bytes.copy_from_slice(&pending_data[48..56]);
                    let activation_time = i64::from_le_bytes(time_bytes);
                    let now = Clock::get()?.unix_timestamp;
                    if now < activation_time {
                        shares
                    } else {
                        0
                    }
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            0
        }
    };

    if pending_locked > 0 {
        let available = ctx.accounts.lp_lp_ata.amount.saturating_sub(pending_locked);
        require!(shares <= available, QuadraticMarketError::SharesStillLocked);
    }

    // Transfer LP tokens to treasury escrow
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.lp_lp_ata.to_account_info(),
                to: ctx.accounts.treasury_lp_ata.to_account_info(),
                authority: ctx.accounts.lp.to_account_info(),
            },
        ),
        shares,
    )?;

    // Snapshot NAV
    let total_reserve = ctx.accounts.treasury_base_ata.amount;
    let free_liquidity = config.free_liquidity(total_reserve);

    let share_price_snapshot = if config.total_lp_supply > 0 {
        ((free_liquidity as u128)
            .checked_mul(SCALE as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?)
        .checked_div(config.total_lp_supply as u128)
        .ok_or(QuadraticMarketError::MathOverflow)? as u64
    } else {
        0
    };

    let now = Clock::get()?.unix_timestamp;
    let cooldown_end = now
        .checked_add(config.withdrawal_cooldown_seconds)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    let req = &mut ctx.accounts.withdrawal_request;
    req.lp = ctx.accounts.lp.key();
    req.shares = shares;
    req.requested_at = now;
    req.cooldown_end = cooldown_end;
    req.nav_snapshot = free_liquidity;
    req.share_price_snapshot = share_price_snapshot;
    req.bump = *ctx.bumps.get("withdrawal_request").unwrap();

    Ok(())
}

// ─── Process Withdrawal ────────────────────────────────────────

#[derive(Accounts)]
pub struct ProcessWithdrawal<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        constraint = lp_mint.key() == global_config.lp_mint @ QuadraticMarketError::Unauthorized,
    )]
    pub lp_mint: Account<'info, Mint>,

    /// CHECK: Treasury PDA
    #[account(
        seeds = [seeds::TREASURY],
        bump = global_config.treasury_bump,
    )]
    pub treasury: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = lp_mint,
        associated_token::authority = treasury,
    )]
    pub treasury_lp_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = withdrawal_request.lp,
    )]
    pub lp_base_ata: Account<'info, TokenAccount>,

    #[account(
        constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized
    )]
    pub base_mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [seeds::WITHDRAWAL, withdrawal_request.lp.as_ref()],
        bump = withdrawal_request.bump,
        close = authority,
    )]
    pub withdrawal_request: Account<'info, WithdrawalRequest>,

    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn process_withdrawal_handler(ctx: Context<ProcessWithdrawal>) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    let req = &ctx.accounts.withdrawal_request;

    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= req.cooldown_end,
        QuadraticMarketError::CooldownNotElapsed
    );

    let total_reserve = ctx.accounts.treasury_base_ata.amount;
    let free_liquidity = config.free_liquidity(total_reserve);

    let current_share_price = if config.total_lp_supply > 0 {
        ((free_liquidity as u128)
            .checked_mul(SCALE as u128)
            .ok_or(QuadraticMarketError::MathOverflow)?)
        .checked_div(config.total_lp_supply as u128)
        .ok_or(QuadraticMarketError::MathOverflow)? as u64
    } else {
        0
    };

    let payout_price = std::cmp::min(req.share_price_snapshot, current_share_price);
    let amount_to_return = ((req.shares as u128)
        .checked_mul(payout_price as u128)
        .ok_or(QuadraticMarketError::MathOverflow)?)
    .checked_div(SCALE as u128)
    .ok_or(QuadraticMarketError::MathOverflow)? as u64;

    require!(
        amount_to_return > 0,
        QuadraticMarketError::InsufficientFreeLiquidity
    );
    require!(
        free_liquidity >= amount_to_return,
        QuadraticMarketError::InsufficientFreeLiquidity
    );

    config.total_lp_supply = config
        .total_lp_supply
        .checked_sub(req.shares)
        .ok_or(QuadraticMarketError::MathUnderflow)?;

    let treasury_seeds = &[seeds::TREASURY, &[config.treasury_bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.treasury_base_ata.to_account_info(),
                to: ctx.accounts.lp_base_ata.to_account_info(),
                authority: ctx.accounts.treasury.to_account_info(),
            },
            &[treasury_seeds],
        ),
        amount_to_return,
    )?;

    token::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Burn {
                mint: ctx.accounts.lp_mint.to_account_info(),
                from: ctx.accounts.treasury_lp_ata.to_account_info(),
                authority: ctx.accounts.treasury.to_account_info(),
            },
            &[treasury_seeds],
        ),
        req.shares,
    )?;

    Ok(())
}

// ─── Activate Liquidity ────────────────────────────────────────

#[derive(Accounts)]
pub struct ActivateLiquidity<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        mut,
        seeds = [seeds::PENDING, pending_liquidity.lp.as_ref()],
        bump = pending_liquidity.bump,
        close = caller,
    )]
    pub pending_liquidity: Account<'info, PendingLiquidity>,

    pub caller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn activate_liquidity_handler(ctx: Context<ActivateLiquidity>) -> Result<()> {
    let pending = &ctx.accounts.pending_liquidity;
    require!(pending.shares > 0, QuadraticMarketError::NoPendingLiquidity);

    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= pending.activation_time,
        QuadraticMarketError::CooldownNotElapsed
    );

    Ok(())
}
