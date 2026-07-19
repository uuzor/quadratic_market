use crate::constants::{seeds, BPS_SCALE, SCALE};
use crate::errors::QuadraticMarketError;
use crate::state::{GlobalConfig, LimitOrder, Market, MarketStatus, OrderSide, OrderStatus};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount};

// ─── Place Order ────────────────────────────────────────────────
//
// Sell order: creator locks outcome tokens in an escrow ATA owned by the
//             order PDA. Tokens are transferred out on fill or returned on cancel.
// Buy order:  creator locks USDC collateral in the treasury. Collateral is
//             released to the filler on fill or returned to creator on cancel.

#[derive(Accounts)]
#[instruction(market_id: u64, outcome_id: u8)]
pub struct PlaceOrder<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        seeds = [seeds::MARKET, market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        init,
        payer = creator,
        space = LimitOrder::LEN,
        seeds = [seeds::ORDER, global_config.next_order_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub order: Box<Account<'info, LimitOrder>>,

    /// CHECK: Treasury PDA — holds USDC collateral for buy orders.
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    // For SELL orders: creator's outcome token ATA (tokens transferred to escrow).
    // For BUY orders:  not required — pass any account, it is ignored.
    #[account(mut)]
    pub creator_outcome_ata: Option<Box<Account<'info, TokenAccount>>>,

    // Escrow ATA for SELL orders: must be owned by the order PDA so only the
    // order PDA can sign transfers out of it. Ownership is validated in the handler.
    // For BUY orders: not required.
    #[account(mut)]
    pub escrow_outcome_ata: Option<Box<Account<'info, TokenAccount>>>,

    // Outcome mint — needed to validate escrow ATA ownership for sell orders.
    #[account(mut)]
    pub outcome_mint: Option<Box<Account<'info, Mint>>>,

    // For BUY orders: creator's USDC ATA (collateral transferred to treasury).
    // For SELL orders: not required.
    #[account(mut)]
    pub creator_base_ata: Option<Box<Account<'info, TokenAccount>>>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = treasury)]
    pub treasury_base_ata: Box<Account<'info, TokenAccount>>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn place_order_handler(
    ctx: Context<PlaceOrder>,
    market_id: u64,
    outcome_id: u8,
    side: OrderSide,
    num_shares: u64,
    price_per_share: u64, // in basis points (10000 = 1.0x)
    expires_at: i64,      // 0 = no expiry
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);

    let market = &ctx.accounts.market;
    require!(
        market.status == MarketStatus::Open,
        QuadraticMarketError::MarketNotOpen
    );
    require!(
        (outcome_id as usize) < market.num_outcomes as usize,
        QuadraticMarketError::InvalidOutcomeId
    );

    let now = Clock::get()?.unix_timestamp;
    // Orders can only be placed while the market is still open for betting
    require!(now < market.start_time, QuadraticMarketError::MarketExpired);

    require!(num_shares > 0, QuadraticMarketError::InvalidAmount);
    require!(
        price_per_share >= BPS_SCALE,
        QuadraticMarketError::InvalidAmount
    );
    if expires_at > 0 {
        require!(expires_at > now, QuadraticMarketError::MarketExpired);
    }

    let mut collateral_locked: u64 = 0;

    match side {
        OrderSide::Sell => {
            // Transfer outcome tokens from creator to escrow ATA.
            let creator_ata = ctx
                .accounts
                .creator_outcome_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            let escrow_ata = ctx
                .accounts
                .escrow_outcome_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

            // Validate that the escrow ATA is owned by the order PDA.
            // The order PDA is derived from [ORDER, next_order_id] — we know the
            // expected key because the order account was just initialised above.
            require!(
                escrow_ata.owner == ctx.accounts.order.key(),
                QuadraticMarketError::InvalidRemainingAccount
            );
            let expected_outcome_mint = market.outcome_mints[outcome_id as usize];
            require!(
                expected_outcome_mint != Pubkey::default(),
                QuadraticMarketError::WrongOutcomeToken
            );
            let outcome_mint = ctx
                .accounts
                .outcome_mint
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            require!(
                outcome_mint.key() == expected_outcome_mint,
                QuadraticMarketError::WrongOutcomeToken
            );
            require!(
                creator_ata.mint == expected_outcome_mint,
                QuadraticMarketError::WrongOutcomeToken
            );
            require!(
                escrow_ata.mint == expected_outcome_mint,
                QuadraticMarketError::WrongOutcomeToken
            );

            require!(
                creator_ata.amount >= num_shares,
                QuadraticMarketError::InsufficientShares
            );

            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: creator_ata.to_account_info(),
                        to: escrow_ata.to_account_info(),
                        authority: ctx.accounts.creator.to_account_info(),
                    },
                ),
                num_shares,
            )?;
        }
        OrderSide::Buy => {
            // Lock USDC collateral = num_shares × price_per_share / BPS_SCALE
            let collateral = num_shares
                .checked_mul(price_per_share)
                .ok_or(QuadraticMarketError::MathOverflow)?
                / BPS_SCALE;
            require!(collateral > 0, QuadraticMarketError::InvalidAmount);

            let creator_base = ctx
                .accounts
                .creator_base_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: creator_base.to_account_info(),
                        to: ctx.accounts.treasury_base_ata.to_account_info(),
                        authority: ctx.accounts.creator.to_account_info(),
                    },
                ),
                collateral,
            )?;

            config.order_collateral_locked = config
                .order_collateral_locked
                .checked_add(collateral)
                .ok_or(QuadraticMarketError::MathOverflow)?;

            collateral_locked = collateral;
        }
    }

    let order_id = config.next_order_id;
    config.next_order_id = config
        .next_order_id
        .checked_add(1)
        .ok_or(QuadraticMarketError::MathOverflow)?;

    let order = &mut ctx.accounts.order;
    order.order_id = order_id;
    order.creator = ctx.accounts.creator.key();
    order.market_id = market_id;
    order.outcome_id = outcome_id;
    order.side = side;
    order.num_shares = num_shares;
    order.filled_shares = 0;
    order.price_per_share = price_per_share;
    order.collateral_locked = collateral_locked;
    order.status = OrderStatus::Open;
    order.created_at = now;
    order.expires_at = expires_at;
    order.bump = *ctx.bumps.get("order").unwrap();

    Ok(())
}

// ─── Fill Order ─────────────────────────────────────────────────
//
// Counterparty fills some or all of an open order.
//
// Sell order fill: filler pays USDC → creator; creator's escrowed tokens → filler.
// Buy order fill:  filler provides outcome tokens → creator; treasury USDC → filler.

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct FillOrder<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::ORDER, order_id.to_le_bytes().as_ref()],
        bump = order.bump,
    )]
    pub order: Box<Account<'info, LimitOrder>>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    // Filler's USDC ATA — used when filling a SELL order (filler pays USDC).
    #[account(mut)]
    pub filler_base_ata: Option<Box<Account<'info, TokenAccount>>>,

    // Creator's USDC ATA — receives USDC when their SELL order is filled.
    #[account(mut)]
    pub creator_base_ata: Option<Box<Account<'info, TokenAccount>>>,

    // Filler's outcome token ATA — used when filling a BUY order (filler provides tokens).
    #[account(mut)]
    pub filler_outcome_ata: Option<Box<Account<'info, TokenAccount>>>,

    // Creator's outcome token ATA — receives tokens when their BUY order is filled.
    #[account(mut)]
    pub creator_outcome_ata: Option<Box<Account<'info, TokenAccount>>>,

    // Escrow ATA holding tokens for SELL orders.
    #[account(mut)]
    pub escrow_outcome_ata: Option<Box<Account<'info, TokenAccount>>>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = treasury)]
    pub treasury_base_ata: Box<Account<'info, TokenAccount>>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Box<Account<'info, Mint>>,

    pub filler: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn fill_order_handler(ctx: Context<FillOrder>, order_id: u64, fill_shares: u64) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    require!(!config.paused, QuadraticMarketError::Paused);

    // Copy scalar fields before any mutable borrow of the order account.
    // The order PDA's AccountInfo is needed as a CPI signer for sell-order escrow
    // transfers, which conflicts with the mutable borrow used to update state.
    let order_fillable = ctx.accounts.order.is_fillable();
    let order_remaining = ctx.accounts.order.remaining_shares();
    let order_expires_at = ctx.accounts.order.expires_at;
    let order_price = ctx.accounts.order.price_per_share;
    let order_side = ctx.accounts.order.side.clone();
    let order_bump = ctx.accounts.order.bump;
    let order_num_shares = ctx.accounts.order.num_shares;

    require!(order_fillable, QuadraticMarketError::OrderNotFillable);
    require!(fill_shares > 0, QuadraticMarketError::InvalidAmount);
    require!(
        fill_shares <= order_remaining,
        QuadraticMarketError::FillExceedsOrder
    );

    let now = Clock::get()?.unix_timestamp;
    if order_expires_at > 0 {
        require!(now < order_expires_at, QuadraticMarketError::OrderExpired);
    }

    // USDC value for this fill: fill_shares × price_per_share
    let fill_cost = ((fill_shares as u128)
        .checked_mul(order_price as u128)
        .ok_or(QuadraticMarketError::MathOverflow)?)
        / SCALE as u128;
    let fill_cost = fill_cost as u64;
    require!(fill_cost > 0, QuadraticMarketError::InvalidAmount);

    let treasury_bump = config.treasury_bump;

    match order_side {
        OrderSide::Sell => {
            // Filler pays USDC to the order creator.
            // Escrowed outcome tokens are released to the filler.
            let filler_base = ctx
                .accounts
                .filler_base_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            let creator_base = ctx
                .accounts
                .creator_base_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            let escrow_ata = ctx
                .accounts
                .escrow_outcome_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            let filler_outcome = ctx
                .accounts
                .filler_outcome_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

            // 1. Filler pays USDC directly to creator (peer-to-peer, no treasury cut)
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: filler_base.to_account_info(),
                        to: creator_base.to_account_info(),
                        authority: ctx.accounts.filler.to_account_info(),
                    },
                ),
                fill_cost,
            )?;

            // 2. Release escrowed outcome tokens to filler.
            // Escrow ATA is owned by the order PDA — sign with order seeds.
            let order_id_bytes = order_id.to_le_bytes();
            let order_seeds: &[&[&[u8]]] =
                &[&[seeds::ORDER, order_id_bytes.as_ref(), &[order_bump]]];
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: escrow_ata.to_account_info(),
                        to: filler_outcome.to_account_info(),
                        // Use the account_info directly — no mutable borrow of order needed
                        authority: ctx.accounts.order.to_account_info(),
                    },
                    order_seeds,
                ),
                fill_shares,
            )?;
        }
        OrderSide::Buy => {
            // Filler provides outcome tokens to the order creator.
            // Treasury releases locked USDC collateral to the filler.
            let filler_outcome = ctx
                .accounts
                .filler_outcome_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            let creator_outcome = ctx
                .accounts
                .creator_outcome_ata
                .as_ref()
                .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
            let (expected_mint_pda, _) = Pubkey::find_program_address(
                &[
                    seeds::OUTCOME_MINT,
                    ctx.accounts.order.market_id.to_le_bytes().as_ref(),
                    ctx.accounts.order.outcome_id.to_le_bytes().as_ref(),
                ],
                &crate::ID,
            );
            require!(
                filler_outcome.mint == expected_mint_pda,
                QuadraticMarketError::WrongOutcomeToken
            );
            require!(
                creator_outcome.mint == expected_mint_pda,
                QuadraticMarketError::WrongOutcomeToken
            );

            // 1. Filler transfers outcome tokens to creator
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: filler_outcome.to_account_info(),
                        to: creator_outcome.to_account_info(),
                        authority: ctx.accounts.filler.to_account_info(),
                    },
                ),
                fill_shares,
            )?;

            // 2. Release collateral from treasury to filler
            let treasury_seeds: &[&[&[u8]]] = &[&[seeds::TREASURY, &[treasury_bump]]];
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.treasury_base_ata.to_account_info(),
                        to: ctx
                            .accounts
                            .filler_base_ata
                            .as_ref()
                            .ok_or(QuadraticMarketError::InvalidRemainingAccount)?
                            .to_account_info(),
                        authority: ctx.accounts.treasury.to_account_info(),
                    },
                    treasury_seeds,
                ),
                fill_cost,
            )?;

            config.order_collateral_locked =
                config.order_collateral_locked.saturating_sub(fill_cost);
            ctx.accounts.order.collateral_locked = ctx
                .accounts
                .order
                .collateral_locked
                .saturating_sub(fill_cost);
        }
    }

    // Now safe to mutably borrow order for state update — all CPIs are done.
    let new_filled = ctx
        .accounts
        .order
        .filled_shares
        .checked_add(fill_shares)
        .ok_or(QuadraticMarketError::MathOverflow)?;
    ctx.accounts.order.filled_shares = new_filled;
    ctx.accounts.order.status = if new_filled >= order_num_shares {
        OrderStatus::Filled
    } else {
        OrderStatus::PartiallyFilled
    };

    Ok(())
}

// ─── Cancel Order ───────────────────────────────────────────────
//
// Creator cancels their order and recovers locked assets.
// Only the order creator can cancel.

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct CancelOrder<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::ORDER, order_id.to_le_bytes().as_ref()],
        bump = order.bump,
        constraint = order.creator == creator.key() @ QuadraticMarketError::Unauthorized,
        close = creator,
    )]
    pub order: Account<'info, LimitOrder>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    // For SELL orders: escrow ATA → creator's outcome ATA.
    #[account(mut)]
    pub escrow_outcome_ata: Option<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub creator_outcome_ata: Option<Account<'info, TokenAccount>>,

    // For BUY orders: treasury USDC → creator's base ATA.
    #[account(mut)]
    pub creator_base_ata: Option<Account<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = treasury)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    #[account(mut)]
    pub creator: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn cancel_order_handler(ctx: Context<CancelOrder>, order_id: u64) -> Result<()> {
    // Copy fields out before any mutable borrows (the `close` constraint on the
    // order account causes Anchor to take a mutable borrow of the account info).
    let order_side = ctx.accounts.order.side.clone();
    let order_bump = ctx.accounts.order.bump;
    let remaining = ctx.accounts.order.remaining_shares();
    let collateral_locked = ctx.accounts.order.collateral_locked;
    let is_cancellable = ctx.accounts.order.is_cancellable();

    require!(is_cancellable, QuadraticMarketError::OrderNotCancellable);

    let config = &mut ctx.accounts.global_config;
    let treasury_bump = config.treasury_bump;

    match order_side {
        OrderSide::Sell => {
            if remaining > 0 {
                let escrow_ata = ctx
                    .accounts
                    .escrow_outcome_ata
                    .as_ref()
                    .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
                let creator_outcome = ctx
                    .accounts
                    .creator_outcome_ata
                    .as_ref()
                    .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

                let order_id_bytes = order_id.to_le_bytes();
                let order_seeds: &[&[&[u8]]] =
                    &[&[seeds::ORDER, order_id_bytes.as_ref(), &[order_bump]]];
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        token::Transfer {
                            from: escrow_ata.to_account_info(),
                            to: creator_outcome.to_account_info(),
                            authority: ctx.accounts.order.to_account_info(),
                        },
                        order_seeds,
                    ),
                    remaining,
                )?;
            }
        }
        OrderSide::Buy => {
            if collateral_locked > 0 {
                let creator_base = ctx
                    .accounts
                    .creator_base_ata
                    .as_ref()
                    .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

                let treasury_seeds: &[&[&[u8]]] = &[&[seeds::TREASURY, &[treasury_bump]]];
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        token::Transfer {
                            from: ctx.accounts.treasury_base_ata.to_account_info(),
                            to: creator_base.to_account_info(),
                            authority: ctx.accounts.treasury.to_account_info(),
                        },
                        treasury_seeds,
                    ),
                    collateral_locked,
                )?;

                config.order_collateral_locked = config
                    .order_collateral_locked
                    .saturating_sub(collateral_locked);
            }
        }
    }

    // order PDA is closed via `close = creator` constraint — rent returned to creator.
    Ok(())
}

// ─── Expire Order ───────────────────────────────────────────────
//
// Permissionless cleanup of orders past their expiry timestamp.
// Caller receives the order PDA rent as an incentive.
// Refund logic is identical to cancel_order.

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct ExpireOrder<'info> {
    #[account(mut, seeds = [seeds::GLOBAL_CONFIG], bump = global_config.bump)]
    pub global_config: Box<Account<'info, GlobalConfig>>,

    #[account(
        mut,
        seeds = [seeds::ORDER, order_id.to_le_bytes().as_ref()],
        bump = order.bump,
        // Rent goes to the caller as incentive for cleanup
        close = caller,
    )]
    pub order: Account<'info, LimitOrder>,

    /// CHECK: Treasury PDA
    #[account(seeds = [seeds::TREASURY], bump = global_config.treasury_bump)]
    pub treasury: SystemAccount<'info>,

    // For SELL orders: escrow → creator's outcome ATA.
    #[account(mut)]
    pub escrow_outcome_ata: Option<Account<'info, TokenAccount>>,

    /// CHECK: Creator's outcome ATA — validated by token program on transfer.
    #[account(mut)]
    pub creator_outcome_ata: Option<Account<'info, TokenAccount>>,

    // For BUY orders: treasury USDC → creator's base ATA.
    /// CHECK: Creator's base ATA — validated by token program on transfer.
    #[account(mut)]
    pub creator_base_ata: Option<Account<'info, TokenAccount>>,

    #[account(mut, associated_token::mint = base_mint, associated_token::authority = treasury)]
    pub treasury_base_ata: Account<'info, TokenAccount>,

    #[account(constraint = base_mint.key() == global_config.base_mint @ QuadraticMarketError::Unauthorized)]
    pub base_mint: Account<'info, Mint>,

    /// Anyone can call expire — they receive the PDA rent.
    #[account(mut)]
    pub caller: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn expire_order_handler(ctx: Context<ExpireOrder>, order_id: u64) -> Result<()> {
    // Copy fields out before any mutable borrows (same reason as cancel_order_handler).
    let order_side = ctx.accounts.order.side.clone();
    let order_bump = ctx.accounts.order.bump;
    let order_expires_at = ctx.accounts.order.expires_at;
    let remaining = ctx.accounts.order.remaining_shares();
    let collateral_locked = ctx.accounts.order.collateral_locked;
    let is_cancellable = ctx.accounts.order.is_cancellable();

    require!(is_cancellable, QuadraticMarketError::OrderNotCancellable);
    require!(order_expires_at > 0, QuadraticMarketError::OrderNotExpired);

    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= order_expires_at,
        QuadraticMarketError::OrderNotExpired
    );

    let config = &mut ctx.accounts.global_config;
    let treasury_bump = config.treasury_bump;

    match order_side {
        OrderSide::Sell => {
            if remaining > 0 {
                let escrow_ata = ctx
                    .accounts
                    .escrow_outcome_ata
                    .as_ref()
                    .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;
                let creator_outcome = ctx
                    .accounts
                    .creator_outcome_ata
                    .as_ref()
                    .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

                let order_id_bytes = order_id.to_le_bytes();
                let order_seeds: &[&[&[u8]]] =
                    &[&[seeds::ORDER, order_id_bytes.as_ref(), &[order_bump]]];
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        token::Transfer {
                            from: escrow_ata.to_account_info(),
                            to: creator_outcome.to_account_info(),
                            authority: ctx.accounts.order.to_account_info(),
                        },
                        order_seeds,
                    ),
                    remaining,
                )?;
            }
        }
        OrderSide::Buy => {
            if collateral_locked > 0 {
                let creator_base = ctx
                    .accounts
                    .creator_base_ata
                    .as_ref()
                    .ok_or(QuadraticMarketError::InvalidRemainingAccount)?;

                let treasury_seeds: &[&[&[u8]]] = &[&[seeds::TREASURY, &[treasury_bump]]];
                token::transfer(
                    CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        token::Transfer {
                            from: ctx.accounts.treasury_base_ata.to_account_info(),
                            to: creator_base.to_account_info(),
                            authority: ctx.accounts.treasury.to_account_info(),
                        },
                        treasury_seeds,
                    ),
                    collateral_locked,
                )?;

                config.order_collateral_locked = config
                    .order_collateral_locked
                    .saturating_sub(collateral_locked);
            }
        }
    }

    // order PDA closed via `close = caller` — rent goes to caller as cleanup incentive.
    Ok(())
}
