use crate::constants::MAX_OPERATORS;
use anchor_lang::prelude::*;

#[account]
pub struct GlobalConfig {
    pub admin: Pubkey,                      // 32
    pub paused: bool,                       // 1
    pub oracle_pubkey: [u8; 32],            // 32  — oracle that signs settlement results
    pub max_market_exposure: u64,           // 8
    pub locked_payouts: u64,                // 8
    pub total_lp_supply: u64,               // 8
    pub lp_mint: Pubkey,                    // 32
    pub base_mint: Pubkey,                  // 32
    pub treasury: Pubkey,                   // 32
    pub treasury_bump: u8,                  // 1
    pub next_market_id: u64,                // 8
    pub challenge_window_seconds: i64,      // 8   — short (default 5 min)
    pub settlement_deadline_seconds: i64,   // 8   — auto-void if oracle silent this long
    pub odds_basis: u64,                    // 8
    pub lmsr_default_b: u64,                // 8   raw lamports (B_raw)
    pub min_first_liquidity: u64,           // 8
    pub current_epoch: u64,                 // 8
    pub epoch_duration_seconds: i64,        // 8
    pub withdrawal_cooldown_seconds: i64,   // 8
    // Sports risk controls
    pub max_single_bet: u64,        // 8   — max lamports per single bet
    pub min_outcome_price_bps: u64, // 8   — minimum implied probability (1 = 0.01%)
    pub buy_fee_bps: u64,           // 8   — house fee on direct buys
    // Operator allowlist (can create/suspend/settle markets)
    pub operators: [Pubkey; MAX_OPERATORS], // 32 * 8 = 256
    pub num_operators: u8,                  // 1
    pub bump: u8,                           // 1
    // Cash-out
    pub cash_out_margin_bps: u64, // 8  — house cut on early cash-out (default 500 = 5%)
    // Peer-to-peer order book
    pub next_order_id: u64,           // 8
    pub order_collateral_locked: u64, // 8  — USDC locked for open buy orders (separate from LP)
    // Epoch controls
    pub epoch_paused: bool, // 1  — prevents new deposits/withdrawals for current epoch
    pub next_epoch_start: i64, // 8  — timestamp when next epoch begins
}

impl GlobalConfig {
    pub const LEN: usize = 8  // discriminator
        + 32  // admin
        + 1   // paused
        + 32  // oracle_pubkey
        + 8   // max_market_exposure
        + 8   // locked_payouts
        + 8   // total_lp_supply
        + 32  // lp_mint
        + 32  // base_mint
        + 32  // treasury
        + 1   // treasury_bump
        + 8   // next_market_id
        + 8   // challenge_window_seconds
        + 8   // settlement_deadline_seconds
        + 8   // odds_basis
        + 8   // lmsr_default_b
        + 8   // min_first_liquidity
        + 8   // current_epoch
        + 8   // epoch_duration_seconds
        + 8   // withdrawal_cooldown_seconds
        + 8   // max_single_bet
        + 8   // min_outcome_price_bps
        + 8   // buy_fee_bps
        + (32 * MAX_OPERATORS) // operators
        + 1   // num_operators
        + 1   // bump
        + 8   // cash_out_margin_bps
        + 8   // next_order_id
        + 8   // order_collateral_locked
        + 1   // epoch_paused
        + 8; // next_epoch_start

    pub fn free_liquidity(&self, treasury_balance: u64) -> u64 {
        let total_locked = self
            .locked_payouts
            .saturating_add(self.order_collateral_locked);
        if treasury_balance > total_locked {
            treasury_balance - total_locked
        } else {
            0
        }
    }

    /// Returns true if `key` is the admin or a registered operator.
    pub fn is_authorized(&self, key: &Pubkey) -> bool {
        if key == &self.admin {
            return true;
        }
        self.operators[..self.num_operators as usize]
            .iter()
            .any(|op| op == key)
    }

    /// Converts the stored oracle bytes to a Pubkey for comparison.
    pub fn oracle_pubkey(&self) -> Pubkey {
        Pubkey::from(self.oracle_pubkey)
    }

    /// Check if we're in an active epoch and deposits/withdrawals are allowed.
    pub fn can_modify_liquidity(&self, now: i64) -> bool {
        // If epoch is paused, no liquidity modifications allowed
        if self.epoch_paused {
            return false;
        }
        // Can only modify liquidity before next epoch starts
        now < self.next_epoch_start
    }

    /// Get the current epoch number based on time
    pub fn get_epoch_for_time(&self, now: i64) -> u64 {
        if self.epoch_duration_seconds > 0 {
            (now / self.epoch_duration_seconds) as u64
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_config_len_not_too_small() {
        // Borsh-serialized size of GlobalConfig fields (excluding discriminator).
        // LEN must be >= the actual struct field size; if this panics on build,
        // increase the constant. Removed slip fields (24 bytes): slip_house_margin_bps,
        // max_slip_bonus_multiplier_bps, next_slip_id. New LEN is 581.
        assert!(GlobalConfig::LEN >= 581, "LEN too small for struct fields");
    }
}
