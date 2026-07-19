//! Settlement with TxLINE on-chain proof validation.
//!
//! This module validates TxLINE proof data by CPI-ing into the official
//! Txoracle program, then settles the market only if the oracle returns `true`.
//!
//! The caller must still be an admin or authorized operator, but the proof
//! itself is verified on-chain against Txoracle.

use crate::constants::seeds;
use crate::errors::QuadraticMarketError;
use crate::state::{Epoch, GlobalConfig, Market, MarketStatus};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::{get_return_data, invoke},
};
use std::str::FromStr;

// TxLINE/Txoracle program IDs
const TXORACLE_DEVNET: &str = "6pW64gN1s2uqjHkn1unFeEjAwJkPGHoppGvS715wyP2J";
const TXORACLE_MAINNET: &str = "9ExbZjAapQww1vfcisDmrngPinHTEfpjYRWMunJgcKaA";
const VALIDATE_STAT_V2_DISCRIMINATOR: [u8; 8] = [208, 215, 194, 214, 241, 71, 246, 178];

/// Proof node for Merkle proof validation
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ProofNode {
    pub hash: [u8; 32],
    pub is_right_sibling: bool,
}

/// TxLINE proof payload types. These mirror the pinned devnet IDL.

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ScoresUpdateStats {
    pub update_count: i32,
    pub min_timestamp: i64,
    pub max_timestamp: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ScoresBatchSummary {
    pub fixture_id: i64,
    pub update_stats: ScoresUpdateStats,
    pub events_subtree_root: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ScoreStat {
    pub key: u32,
    pub value: i32,
    pub period: i32,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct StatLeaf {
    pub stat: ScoreStat,
    pub stat_proof: Vec<ProofNode>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct StatValidationInput {
    pub ts: i64,
    pub fixture_summary: ScoresBatchSummary,
    pub fixture_proof: Vec<ProofNode>,
    pub main_tree_proof: Vec<ProofNode>,
    pub event_stat_root: [u8; 32],
    pub stats: Vec<StatLeaf>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct GeometricTarget {
    pub stat_index: u8,
    pub prediction: i32,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct TraderPredicate {
    pub threshold: i32,
    pub comparison: Comparison,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum Comparison {
    EqualTo,
    GreaterThan,
    LessThan,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum BinaryExpression {
    Add,
    Subtract,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum StatPredicate {
    Single {
        index: u8,
        predicate: TraderPredicate,
    },
    Binary {
        index_a: u8,
        index_b: u8,
        op: BinaryExpression,
        predicate: TraderPredicate,
    },
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct NDimensionalStrategy {
    pub geometric_targets: Vec<GeometricTarget>,
    pub distance_predicate: Option<TraderPredicate>,
    pub discrete_predicates: Vec<StatPredicate>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
struct ValidateStatV2Args {
    payload: StatValidationInput,
    strategy: NDimensionalStrategy,
}

fn build_validate_stat_v2_ix(
    txoracle_program: Pubkey,
    daily_scores_merkle_roots: Pubkey,
    payload: StatValidationInput,
    strategy: NDimensionalStrategy,
) -> Result<Instruction> {
    let mut data = Vec::with_capacity(8 + 512);
    data.extend_from_slice(&VALIDATE_STAT_V2_DISCRIMINATOR);
    data.extend_from_slice(
        &ValidateStatV2Args {
            payload,
            strategy,
        }
        .try_to_vec()
        .map_err(|_| error!(QuadraticMarketError::MathOverflow))?,
    );

    Ok(Instruction {
        program_id: txoracle_program,
        accounts: vec![AccountMeta::new_readonly(daily_scores_merkle_roots, false)],
        data,
    })
}

fn read_txoracle_bool_return(expected_program_id: Pubkey) -> Result<bool> {
    let Some((program_id, data)) = get_return_data() else {
        return Err(error!(QuadraticMarketError::TxlineProofValidationFailed));
    };

    require!(
        program_id == expected_program_id,
        QuadraticMarketError::TxlineProofValidationFailed
    );

    let mut bytes: &[u8] = &data;
    bool::deserialize(&mut bytes).map_err(|_| error!(QuadraticMarketError::TxlineProofValidationFailed))
}

fn is_supported_txoracle_program(program_id: &Pubkey) -> bool {
    matches!(Pubkey::from_str(TXORACLE_DEVNET), Ok(devnet) if &devnet == program_id)
        || matches!(Pubkey::from_str(TXORACLE_MAINNET), Ok(mainnet) if &mainnet == program_id)
}

pub(crate) fn validate_txoracle_proof(
    txoracle_program: &AccountInfo<'_>,
    daily_scores_merkle_roots: &AccountInfo<'_>,
    payload: StatValidationInput,
    strategy: NDimensionalStrategy,
) -> Result<()> {
    let txoracle_program_id = *txoracle_program.key;
    require!(
        is_supported_txoracle_program(&txoracle_program_id),
        QuadraticMarketError::TxlineProofValidationFailed
    );
    require!(
        txoracle_program.executable,
        QuadraticMarketError::TxlineProofValidationFailed
    );

    let ix = build_validate_stat_v2_ix(
        txoracle_program_id,
        *daily_scores_merkle_roots.key,
        payload,
        strategy,
    )?;
    invoke(&ix, &[daily_scores_merkle_roots.clone()])
        .map_err(|_| error!(QuadraticMarketError::TxlineProofValidationFailed))?;

    require!(
        read_txoracle_bool_return(txoracle_program_id)?,
        QuadraticMarketError::TxlineProofValidationFailed
    );

    Ok(())
}

/// Settlement with TxLINE proof - role gated
///
/// Only authorized callers can submit valid TxLINE proof data.
/// The proof is validated on-chain via CPI to the Txoracle program.
#[derive(Accounts)]
#[instruction(market_id: u64)]
pub struct SettleWithProof<'info> {
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
        constraint = market.market_id == market_id @ QuadraticMarketError::InvalidAmount,
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        mut,
        seeds = [seeds::EPOCH, market.epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
        constraint = epoch.epoch_id == market.epoch_id @ QuadraticMarketError::EpochAccountMismatch,
    )]
    pub epoch: Account<'info, Epoch>,

    /// PDA for daily scores Merkle roots.
    /// CHECK: Read-only CPI account required by Txoracle.
    pub daily_scores_merkle_roots: AccountInfo<'info>,

    /// CHECK: Txoracle program account.
    pub txoracle_program: AccountInfo<'info>,

    /// The caller (must be an admin or authorized operator)
    pub caller: Signer<'info>,
}

/// Settle with proof after validating the Txoracle proof bundle on-chain.
pub fn settle_with_proof_handler(
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
    let config = &ctx.accounts.global_config;
    let market = &mut ctx.accounts.market;
    let epoch = &mut ctx.accounts.epoch;

    // Only approved roles can submit proof-based settlement.
    require!(
        config.is_authorized(&ctx.accounts.caller.key()),
        QuadraticMarketError::Unauthorized
    );

    // Verify market can be settled.
    require!(
        market.status.can_settle(),
        QuadraticMarketError::InvalidMarketStatus
    );

    // Bind the provided proof payload to the market being settled.
    if let Some(fixture_id) = market.txline_fixture_id {
        require!(
            fixture_id == txline_fixture_id,
            QuadraticMarketError::InvalidTxlineFixtureId
        );
    }
    require!(
        validation_input.fixture_summary.fixture_id == txline_fixture_id as i64,
        QuadraticMarketError::InvalidTxlineFixtureId
    );
    require!(
        validation_input.ts == validation_timestamp,
        QuadraticMarketError::TxlineProofValidationFailed
    );

    // Validate outcome.
    require!(
        (proposed_outcome as usize) < market.num_outcomes as usize,
        QuadraticMarketError::InvalidProposedOutcome
    );

    // Verify match has started
    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= market.start_time,
        QuadraticMarketError::MarketAlreadyStarted
    );

    // Call the real Txoracle validate_stat_v2 CPI.
    validate_txoracle_proof(
        &ctx.accounts.txoracle_program,
        &ctx.accounts.daily_scores_merkle_roots,
        validation_input,
        strategy,
    )?;

    // Derive winning outcome from scores based on market type
    let winning_outcome = derive_winning_outcome(
        home_score,
        away_score,
        &market.market_type,
    )?;
    require!(
        winning_outcome == proposed_outcome,
        QuadraticMarketError::InvalidProposedOutcome
    );

    // Mark market as settled
    market.winning_outcome = winning_outcome;
    market.status = MarketStatus::Settled;
    market.settlement_time = now;
    market.txline_proof_verified = true;

    // Update epoch settlement tracking
    if !market.settled_in_epoch {
        market.settled_in_epoch = true;
        epoch.num_settled_markets = epoch
            .num_settled_markets
            .checked_add(1)
            .ok_or(QuadraticMarketError::MathOverflow)?;

        // When all markets settled, enable LP withdrawals
        if epoch.num_markets > 0 && epoch.num_settled_markets >= epoch.num_markets {
            epoch.all_markets_settled = true;
            epoch.lp_shares_at_close = config.total_lp_supply;
            epoch.withdrawals_enabled = true;
        }
    }

    emit!(MarketSettledWithProof {
        market_id,
        winning_outcome,
        txline_fixture_id,
        validation_timestamp,
        home_score,
        away_score,
        caller: ctx.accounts.caller.key(),
    });

    Ok(())
}

/// Derive winning outcome from scores based on market type
fn derive_winning_outcome(
    home_score: i64,
    away_score: i64,
    market_type: &crate::state::market::MarketType,
) -> Result<u8> {
    use crate::state::market::MarketType;

    match market_type {
        MarketType::OneXTwo => {
            // 1X2: 0=home win, 1=draw, 2=away win
            if home_score > away_score {
                Ok(0) // Home wins
            } else if home_score < away_score {
                Ok(2) // Away wins
            } else {
                Ok(1) // Draw
            }
        }
        MarketType::OverUnder => {
            // O/U 2.5: 0=over (total > 2.5), 1=under (total <= 2.5)
            let total = home_score.saturating_add(away_score);
            if total > 2 {
                Ok(0) // Over 2.5
            } else {
                Ok(1) // Under 2.5
            }
        }
        MarketType::GoalNoGoal => {
            // GG/NG: 0=GG (both score), 1=NG (one or neither scores)
            if home_score > 0 && away_score > 0 {
                Ok(0) // GG
            } else {
                Ok(1) // NG
            }
        }
    }
}

/// Events for proof-based settlement
#[event]
pub struct MarketSettledWithProof {
    pub market_id: u64,
    pub winning_outcome: u8,
    pub txline_fixture_id: u64,
    pub validation_timestamp: i64,
    pub home_score: i64,
    pub away_score: i64,
    pub caller: Pubkey,
}

// ─── Full Txoracle Integration (Future) ───────────────────────────
//
// For full on-chain validation, implement CPI to Txoracle:
//
// #[derive(Accounts)]
// pub struct SettleWithValidatedProof<'info> {
//     pub txoracle_program: Program<'info, Txoracle>,
//     // ... other accounts ...
// }
//
// pub fn settle_with_validated_proof_handler(
//     ctx: Context<SettleWithValidatedProof>,
//     proof_data: ProofData,
// ) -> Result<()> {
//     // 1. Call txoracle.validateStat via CPI
//     // 2. Check validation result
//     // 3. If valid, settle market
// }
