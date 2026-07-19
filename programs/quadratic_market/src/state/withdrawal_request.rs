use anchor_lang::prelude::*;

#[account]
pub struct WithdrawalRequest {
    pub lp: Pubkey,                // 32
    pub shares: u64,               // 8
    pub requested_at: i64,         // 8
    pub cooldown_end: i64,         // 8 — earliest time this can be processed
    pub nav_snapshot: u64,         // 8 — free_liquidity at request time
    pub share_price_snapshot: u64, // 8 — share price (Q32.32) at request time
    pub bump: u8,                  // 1
}

impl WithdrawalRequest {
    pub const LEN: usize = 8 + 32 + 8 + 8 + 8 + 8 + 8 + 1; // 81
}
