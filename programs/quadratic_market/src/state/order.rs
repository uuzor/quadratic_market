use anchor_lang::prelude::*;

/// Which side of the order book the creator is on.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum OrderSide {
    /// Creator holds outcome tokens and wants to exit for USDC.
    Sell,
    /// Creator wants to acquire outcome tokens and locks USDC upfront.
    Buy,
}

/// Lifecycle state of a limit order.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
}

impl Default for OrderStatus {
    fn default() -> Self {
        OrderStatus::Open
    }
}

/// A peer-to-peer limit order for outcome tokens.
///
/// Sell orders: creator locks outcome tokens in escrow at placement.
/// Buy orders:  creator locks USDC collateral in treasury at placement.
/// Either side can be partially filled. The creator can cancel at any time
/// to recover locked assets. Anyone can expire an order past its deadline.
#[account]
pub struct LimitOrder {
    pub order_id: u64,          // 8
    pub creator: Pubkey,        // 32
    pub market_id: u64,         // 8
    pub outcome_id: u8,         // 1
    pub side: OrderSide,        // 1
    pub num_shares: u64,        // 8  — total shares in order
    pub filled_shares: u64,     // 8  — shares matched so far
    pub price_per_share: u64,   // 8  — Q32.32 implied probability (e.g. 0.5 * SCALE = 50%)
    pub collateral_locked: u64, // 8  — USDC locked for buy orders; 0 for sell orders
    pub status: OrderStatus,    // 1
    pub created_at: i64,        // 8
    pub expires_at: i64,        // 8  — 0 = no expiry
    pub bump: u8,               // 1
}

impl LimitOrder {
    pub const LEN: usize = 8   // discriminator
        + 8   // order_id
        + 32  // creator
        + 8   // market_id
        + 1   // outcome_id
        + 1   // side
        + 8   // num_shares
        + 8   // filled_shares
        + 8   // price_per_share
        + 8   // collateral_locked
        + 1   // status
        + 8   // created_at
        + 8   // expires_at
        + 1   // bump
        + 6; // padding

    /// Shares remaining to be filled.
    pub fn remaining_shares(&self) -> u64 {
        self.num_shares.saturating_sub(self.filled_shares)
    }

    /// True if the order can accept new fills.
    pub fn is_fillable(&self) -> bool {
        matches!(
            self.status,
            OrderStatus::Open | OrderStatus::PartiallyFilled
        )
    }

    /// True if the order can be cancelled by its creator.
    pub fn is_cancellable(&self) -> bool {
        matches!(
            self.status,
            OrderStatus::Open | OrderStatus::PartiallyFilled
        )
    }
}
