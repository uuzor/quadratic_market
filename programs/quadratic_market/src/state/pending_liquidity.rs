use anchor_lang::prelude::*;

/// Tracks LP shares that are minted but locked until activation_time.
/// Shares count toward total_lp_supply from deposit time (invariant-safe),
/// but cannot be used for withdrawal until the activation delay passes.
#[account]
pub struct PendingLiquidity {
    pub lp: Pubkey,            // 32 — depositor
    pub shares: u64,           // 8  — locked LP shares (accumulates across deposits)
    pub activation_time: i64,  // 8  — fixed timestamp when shares unlock
    pub amount_deposited: u64, // 8  — total base tokens deposited (auditing)
    pub bump: u8,              // 1
}

impl PendingLiquidity {
    // Borsh sequential: 8 + 32 + 8 + 8 + 8 + 1 = 65
    pub const LEN: usize = 8 + 32 + 8 + 8 + 8 + 1;
}
