use crate::constants::{seeds, MAX_GROUP_MARKETS};
use crate::errors::QuadraticMarketError;
use crate::state::{GlobalConfig, Market, MarketGroup};
use anchor_lang::prelude::*;

// ─── Create Market Group ───────────────────────────────────────

#[derive(Accounts)]
#[instruction(group_id: u64)]
pub struct CreateMarketGroup<'info> {
    #[account(
        mut,
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        init,
        payer = creator,
        space = MarketGroup::LEN,
        seeds = [seeds::MARKET_GROUP, group_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub market_group: Account<'info, MarketGroup>,

    #[account(mut)]
    pub creator: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn create_market_group_handler(
    ctx: Context<CreateMarketGroup>,
    group_id: u64,
    max_group_exposure: u64,
    event_start_time: i64,
    title: String,
) -> Result<()> {
    let config = &ctx.accounts.global_config;
    require!(
        ctx.accounts.creator.key() == config.admin,
        QuadraticMarketError::Unauthorized
    );

    let now = Clock::get()?.unix_timestamp;
    require!(
        event_start_time > now,
        QuadraticMarketError::GroupEventStarted
    );

    let group = &mut ctx.accounts.market_group;
    group.group_id = group_id;
    group.creator = ctx.accounts.creator.key();
    group.total_group_exposure = 0;
    group.max_group_exposure = max_group_exposure;
    group.num_markets = 0;
    group.market_ids = [0u64; MAX_GROUP_MARKETS];
    group.event_start_time = event_start_time;
    group.title = title;
    group.bump = *ctx.bumps.get("market_group").unwrap();

    Ok(())
}

// ─── Add Market to Group ──────────────────────────────────────
// Links a market (1X2, O/U, or GG/NG) to a group for tracking purposes.
// Each market settles independently with its own oracle submission.

#[derive(Accounts)]
#[instruction(group_id: u64, market_index: u8)]
pub struct AddMarketToGroup<'info> {
    #[account(
        seeds = [seeds::GLOBAL_CONFIG],
        bump = global_config.bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        mut,
        seeds = [seeds::MARKET_GROUP, group_id.to_le_bytes().as_ref()],
        bump = market_group.bump,
    )]
    pub market_group: Account<'info, MarketGroup>,

    #[account(
        mut,
        seeds = [seeds::MARKET, market.market_id.to_le_bytes().as_ref()],
        bump = market.bump,
    )]
    pub market: Account<'info, Market>,

    pub authority: Signer<'info>,
}

pub fn add_market_to_group_handler(
    ctx: Context<AddMarketToGroup>,
    group_id: u64,
    market_index: u8,
) -> Result<()> {
    let config = &ctx.accounts.global_config;
    let group = &mut ctx.accounts.market_group;
    let market = &mut ctx.accounts.market;

    require!(
        ctx.accounts.authority.key() == config.admin,
        QuadraticMarketError::Unauthorized
    );

    require!(
        market.group_id.is_none(),
        QuadraticMarketError::MarketAlreadyInGroup
    );

    require!(
        (group.num_markets as usize) < MAX_GROUP_MARKETS,
        QuadraticMarketError::MarketGroupFull
    );

    let now = Clock::get()?.unix_timestamp;
    require!(
        now < group.event_start_time,
        QuadraticMarketError::GroupEventStarted
    );

    require!(
        (market_index as usize) < MAX_GROUP_MARKETS,
        QuadraticMarketError::InvalidAmount
    );

    // Link market to group
    market.group_id = Some(group_id);

    // Add market to group's list
    let idx = group.num_markets as usize;
    let mid = market.market_id;
    group.market_ids[idx] = mid;
    group.num_markets += 1;

    Ok(())
}
