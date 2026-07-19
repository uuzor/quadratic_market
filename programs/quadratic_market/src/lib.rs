use anchor_lang::prelude::*;

declare_id!("4wKXu91KW6EBiecjUUYupQHjab6AULrGCm6hNrWbAvaA");

pub mod constants;
pub mod errors;
pub mod math;
pub mod state;
pub mod utils;

// Instruction modules
pub mod admin;
pub mod claim;
pub mod epoch_ops;
pub mod initialize;
pub mod liquidity;
pub mod market_group;
pub mod market_ops;
pub mod orders;
pub mod settlement_with_proof;
pub mod slip;
pub mod trade;

// Bring all account structs into scope so Anchor's #[program]
// macro references them directly
use crate::state::bet_slip::SlipLeg;
use crate::state::market::MarketType;
use crate::state::order::OrderSide;
use admin::*;
use claim::*;
use epoch_ops::*;
use initialize::*;
use market_group::*;
use market_ops::*;
use orders::*;
use settlement_with_proof::*;
use slip::*;

#[program]
pub mod quadratic_market {
    use super::*;

    // ─── Initialization ───────────────────────────────────────

    pub fn initialize(
        ctx: Context<Initialize>,
        oracle_pubkey: [u8; 32],
        max_market_exposure: u64,
    ) -> Result<()> {
        handler(ctx, oracle_pubkey, max_market_exposure)
    }

    // ─── Admin ────────────────────────────────────────────────

    pub fn transfer_admin(ctx: Context<TransferAdmin>, new_admin: Pubkey) -> Result<()> {
        transfer_admin_handler(ctx, new_admin)
    }

    pub fn pause(ctx: Context<Pause>) -> Result<()> {
        pause_handler(ctx)
    }

    pub fn unpause(ctx: Context<Unpause>) -> Result<()> {
        unpause_handler(ctx)
    }

    pub fn update_config(
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
        update_config_handler(
            ctx,
            max_market_exposure,
            challenge_window_seconds,
            settlement_deadline_seconds,
            epoch_duration_seconds,
            withdrawal_cooldown_seconds,
            max_single_bet,
            min_odds_bps,
            max_odds_bps,
            house_fee_bps,
            oracle_pubkey,
        )
    }

    pub fn add_operator(ctx: Context<AddOperator>, operator: Pubkey) -> Result<()> {
        add_operator_handler(ctx, operator)
    }

    pub fn remove_operator(ctx: Context<RemoveOperator>, operator: Pubkey) -> Result<()> {
        remove_operator_handler(ctx, operator)
    }

    // ─── Market Operations ────────────────────────────────────

    pub fn create_market(
        ctx: Context<CreateMarket>,
        start_time: i64,
        num_outcomes: u8,
        title: String,
        description: String,
        category: u8,
        market_type: MarketType,
        initial_odds: Vec<u64>,
        txline_fixture_id: Option<u64>,
    ) -> Result<()> {
        create_market_handler(
            ctx,
            start_time,
            num_outcomes,
            title,
            description,
            category,
            market_type,
            initial_odds,
            txline_fixture_id,
        )
    }

    pub fn init_outcome_mint(
        ctx: Context<InitOutcomeMint>,
        market_id: u64,
        outcome_id: u8,
    ) -> Result<()> {
        init_outcome_mint_handler(ctx, market_id, outcome_id)
    }

    pub fn suspend_market(ctx: Context<SuspendMarket>) -> Result<()> {
        suspend_market_handler(ctx)
    }

    pub fn resume_market(ctx: Context<ResumeMarket>) -> Result<()> {
        resume_market_handler(ctx)
    }

    pub fn void_market(ctx: Context<VoidMarket>) -> Result<()> {
        void_market_handler(ctx)
    }

    pub fn void_if_expired(ctx: Context<VoidIfExpired>) -> Result<()> {
        void_if_expired_handler(ctx)
    }

    // ─── Betting via Slip Only ─────────────────────────────────
    // All betting goes through the slip system. Use place_slip_await for single or multi-leg bets.
    // Backend executes each leg via buy_leg_for_slip as separate transactions.

    // ─── TxLINE Proof-Based Settlement ─────────────────────────────
    // Role-gated settlement using TxLINE Merkle proofs

    /// Settle market using TxLINE on-chain proof validation.
    /// Only authorized operators/admin can call this with valid proof data.
    ///
    /// Flow:
    /// 1. Bot fetches proof from TxLINE API
    /// 2. Bot calls this instruction with Txoracle payload + strategy
    /// 3. Program validates proof via CPI to Txoracle
    /// 4. Market is settled with the derived outcome
    pub fn settle_with_proof(
        ctx: Context<SettleWithProof>,
        market_id: u64,
        proposed_outcome: u8,
        txline_fixture_id: u64,
        validation_timestamp: i64,
        home_score: i64,
        away_score: i64,
        validation_input: StatValidationInput,
        strategy: NDimensionalStrategy,
    ) -> Result<()> {
        settle_with_proof_handler(
            ctx,
            market_id,
            proposed_outcome,
            txline_fixture_id,
            validation_timestamp,
            home_score,
            away_score,
            validation_input,
            strategy,
        )
    }

    // ─── Claims ───────────────────────────────────────────────

    pub fn claim_payout(ctx: Context<ClaimPayout>, market_id: u64) -> Result<()> {
        claim_payout_handler(ctx, market_id)
    }

    pub fn close_market(ctx: Context<CloseMarket>, market_id: u64) -> Result<()> {
        close_market_handler(ctx, market_id)
    }

    /// Refund a user's original stake when the protocol is paused.
    pub fn claim_paused_bet(ctx: Context<ClaimPausedBet>, slip_id: u64) -> Result<()> {
        claim_paused_bet_handler(ctx, slip_id)
    }

    // ─── Market Group Operations ────────────────────────────────
    // Note: Market groups are now just for tracking purposes.
    // Each market (1X2, O/U, GG/NG) settles independently.

    pub fn create_market_group(
        ctx: Context<CreateMarketGroup>,
        group_id: u64,
        max_group_exposure: u64,
        event_start_time: i64,
        title: String,
    ) -> Result<()> {
        create_market_group_handler(ctx, group_id, max_group_exposure, event_start_time, title)
    }

    pub fn add_market_to_group(
        ctx: Context<AddMarketToGroup>,
        group_id: u64,
        market_index: u8,
    ) -> Result<()> {
        add_market_to_group_handler(ctx, group_id, market_index)
    }

    // ─── Update Market Odds ─────────────────────────────────────
    // Can update odds until match starts

    pub fn update_market_odds(
        ctx: Context<UpdateMarketOdds>,
        market_id: u64,
        new_odds: Vec<u64>,
    ) -> Result<()> {
        update_market_odds_handler(ctx, market_id, new_odds)
    }

    /// Update market odds after validating a TxLINE proof bundle on-chain.
    pub fn update_market_odds_with_proof(
        ctx: Context<UpdateMarketOddsWithProof>,
        market_id: u64,
        new_odds: Vec<u64>,
        validation_input: StatValidationInput,
        strategy: NDimensionalStrategy,
    ) -> Result<()> {
        update_market_odds_with_proof_handler(
            ctx,
            market_id,
            new_odds,
            validation_input,
            strategy,
        )
    }

    // ─── Bet Slip (New Decomposed System) ───────────────────────

    /// Place slip await: escrows stake, records legs, locks fixed odds.
    /// Backend then fires N × buy_leg_for_slip.
    pub fn place_slip_await<'info>(
        ctx: Context<'_, '_, '_, 'info, PlaceSlipAwait<'info>>,
        legs: Vec<SlipLeg>,
        stake: u64,
        cancel_deadline: i64,
    ) -> Result<()> {
        place_slip_await_handler(ctx, legs, stake, cancel_deadline)
    }

    /// Buy one leg for slip. Backend calls this N times after place_slip_await.
    pub fn buy_leg_for_slip<'info>(
        ctx: Context<'_, '_, '_, 'info, BuyLegForSlip<'info>>,
        slip_id: u64,
        leg_index: u8,
    ) -> Result<()> {
        buy_leg_for_slip_handler(ctx, slip_id, leg_index)
    }

    /// Cancel slip if deadline passed or legs not bought.
    pub fn cancel_slip<'info>(
        ctx: Context<'_, '_, '_, 'info, CancelSlip<'info>>,
        slip_id: u64,
    ) -> Result<()> {
        cancel_slip_handler(ctx, slip_id)
    }

    /// Settle one leg of a slip. Permissionless.
    pub fn settle_slip_leg<'info>(
        ctx: Context<'_, '_, '_, 'info, SettleSlipLeg<'info>>,
        slip_id: u64,
        leg_index: u8,
    ) -> Result<()> {
        settle_slip_leg_handler(ctx, slip_id, leg_index)
    }

    /// Resolve slip: finalize payout after all legs settled.
    pub fn resolve_slip<'info>(
        ctx: Context<'_, '_, '_, 'info, ResolveSlip<'info>>,
        slip_id: u64,
    ) -> Result<()> {
        resolve_slip_handler(ctx, slip_id)
    }

    // ─── Peer-to-Peer Order Book ────────────────────────────────

    pub fn place_order(
        ctx: Context<PlaceOrder>,
        market_id: u64,
        outcome_id: u8,
        side: OrderSide,
        num_shares: u64,
        price_per_share: u64,
        expires_at: i64,
    ) -> Result<()> {
        place_order_handler(
            ctx,
            market_id,
            outcome_id,
            side,
            num_shares,
            price_per_share,
            expires_at,
        )
    }

    pub fn fill_order(ctx: Context<FillOrder>, order_id: u64, fill_shares: u64) -> Result<()> {
        fill_order_handler(ctx, order_id, fill_shares)
    }

    pub fn cancel_order(ctx: Context<CancelOrder>, order_id: u64) -> Result<()> {
        cancel_order_handler(ctx, order_id)
    }

    pub fn expire_order(ctx: Context<ExpireOrder>, order_id: u64) -> Result<()> {
        expire_order_handler(ctx, order_id)
    }

    // ─── Epoch Management ─────────────────────────────────────────

    /// Create the on-chain Epoch account for the current epoch.
    /// Must be called before any markets can be created in a new epoch.
    pub fn init_epoch(ctx: Context<InitEpoch>) -> Result<()> {
        init_epoch_handler(ctx)
    }

    /// Pause epoch — blocks deposits, withdrawals, and market creation.
    /// Admin can pause at any time (e.g. between epochs or for emergency).
    pub fn pause_epoch(ctx: Context<PauseEpoch>) -> Result<()> {
        pause_epoch_handler(ctx)
    }

    /// Unpause epoch — re-enables deposits, withdrawals, and market creation.
    pub fn unpause_epoch(ctx: Context<UnpauseEpoch>) -> Result<()> {
        unpause_epoch_handler(ctx)
    }

    /// Manually close an epoch and enable LP withdrawals.
    /// Normally auto-triggered when the last market in the epoch settles.
    pub fn close_epoch(ctx: Context<CloseEpoch>) -> Result<()> {
        close_epoch_handler(ctx)
    }

    // ─── Epoch Vault Operations (Step 2) ────────────────────────

    /// Publish an epoch and its vault — announcement LPs see before opting in.
    pub fn publish_epoch(
        ctx: Context<PublishEpoch>,
        epoch_id: u64,
        market_ids: Vec<u64>,
    ) -> Result<()> {
        publish_epoch_handler(ctx, epoch_id, market_ids)
    }

    /// Opt into an epoch's liquidity pool.
    pub fn opt_in_epoch_liquidity(
        ctx: Context<OptInEpochLiquidity>,
        epoch_id: u64,
        amount: u64,
    ) -> Result<()> {
        opt_in_epoch_liquidity_handler(ctx, epoch_id, amount)
    }

    /// Withdraw liquidity after epoch settlement.
    pub fn withdraw_epoch_liquidity(
        ctx: Context<WithdrawEpochLiquidity>,
        epoch_id: u64,
        shares: u64,
    ) -> Result<()> {
        withdraw_epoch_liquidity_handler(ctx, epoch_id, shares)
    }

    /// Enable withdrawals on an epoch vault (called when epoch settles).
    pub fn enable_epoch_withdrawals(
        ctx: Context<EnableEpochWithdrawals>,
        epoch_id: u64,
    ) -> Result<()> {
        enable_epoch_withdrawals_handler(ctx, epoch_id)
    }
}
