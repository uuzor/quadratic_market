use anchor_lang::prelude::*;

/// Tracks the state of an epoch — a time-bounded period during which
/// markets are created and settled. LPs can only withdraw after their
/// epoch's markets have all settled.
#[account]
pub struct Epoch {
    pub epoch_id: u64,                // 8  — unique epoch identifier
    pub start_time: i64,              // 8  — when this epoch began
    pub end_time: i64,                // 8  — when this epoch ends (start + duration)
    pub total_liquidity_added: u64,   // 8  — total base tokens added during this epoch
    pub total_liquidity_removed: u64, // 8  — total base tokens withdrawn after settlement
    pub num_markets: u16,             // 2  — number of markets created in this epoch
    pub num_settled_markets: u16,     // 2  — number of markets that have been settled
    pub all_markets_settled: bool,    // 1  — true when all markets in epoch are settled
    pub withdrawals_enabled: bool,    // 1  — true when LPs can withdraw for this epoch
    pub lp_shares_at_close: u64,      // 8  — total LP supply when epoch closed (for NAV)
    pub bump: u8,                     // 1
}

impl Epoch {
    pub const LEN: usize = 8   // discriminator
        + 8   // epoch_id
        + 8   // start_time
        + 8   // end_time
        + 8   // total_liquidity_added
        + 8   // total_liquidity_removed
        + 2   // num_markets
        + 2   // num_settled_markets
        + 1   // all_markets_settled
        + 1   // withdrawals_enabled
        + 8   // lp_shares_at_close
        + 1; // bump

    /// Returns true if all markets in this epoch have been settled.
    /// Also sets `all_markets_settled` as a side-effect.
    pub fn check_all_settled(&mut self) -> bool {
        if self.num_markets > 0 && self.num_settled_markets >= self.num_markets {
            self.all_markets_settled = true;
            true
        } else {
            false
        }
    }

    /// Returns true if the epoch is currently active (started but not ended).
    pub fn is_active(&self, now: i64) -> bool {
        now >= self.start_time && now < self.end_time
    }

    /// Returns true if the epoch has expired (current time is past end_time).
    pub fn is_expired(&self, now: i64) -> bool {
        now >= self.end_time
    }

    /// Returns true if withdrawals are allowed for this epoch.
    pub fn can_withdraw(&self) -> bool {
        self.withdrawals_enabled && self.all_markets_settled
    }

    /// Returns the time remaining until the epoch starts (negative if already started).
    pub fn time_until_start(&self, now: i64) -> i64 {
        self.start_time.saturating_sub(now)
    }

    /// Returns the time remaining until the epoch ends (negative if already ended).
    pub fn time_until_end(&self, now: i64) -> i64 {
        self.end_time.saturating_sub(now)
    }

    /// Returns the number of markets still pending settlement.
    pub fn pending_settlements(&self) -> u16 {
        self.num_markets.saturating_sub(self.num_settled_markets)
    }

    /// Returns the settlement progress as basis points (0-10000 for 0.00%-100.00%).
    pub fn settlement_progress_bps(&self) -> u16 {
        if self.num_markets == 0 {
            return 10000; // Fully settled if no markets
        }
        (self.num_settled_markets as u32 * 10000 / self.num_markets as u32) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_epoch(
        epoch_id: u64,
        start_time: i64,
        end_time: i64,
        num_markets: u16,
        num_settled: u16,
        withdrawals_enabled: bool,
    ) -> Epoch {
        Epoch {
            epoch_id,
            start_time,
            end_time,
            total_liquidity_added: 0,
            total_liquidity_removed: 0,
            num_markets,
            num_settled_markets: num_settled,
            all_markets_settled: num_settled >= num_markets && num_markets > 0,
            withdrawals_enabled,
            lp_shares_at_close: 0,
            bump: 1,
        }
    }

    #[test]
    fn test_is_active() {
        let epoch = create_test_epoch(1, 1000, 2000, 5, 0, false);

        assert!(epoch.is_active(1500)); // During epoch
        assert!(!epoch.is_active(500)); // Before epoch
        assert!(!epoch.is_active(2500)); // After epoch
    }

    #[test]
    fn test_is_expired() {
        let epoch = create_test_epoch(1, 1000, 2000, 5, 0, false);

        assert!(!epoch.is_expired(1500)); // During epoch
        assert!(epoch.is_expired(2500)); // After epoch
    }

    #[test]
    fn test_can_withdraw() {
        let mut epoch_active = create_test_epoch(1, 1000, 2000, 5, 5, true);
        epoch_active.all_markets_settled = true;
        assert!(epoch_active.can_withdraw());

        let mut epoch_no_withdrawals = create_test_epoch(1, 1000, 2000, 5, 5, false);
        epoch_no_withdrawals.all_markets_settled = true;
        assert!(!epoch_no_withdrawals.can_withdraw());
    }

    #[test]
    fn test_pending_settlements() {
        let epoch = create_test_epoch(1, 1000, 2000, 10, 3, false);
        assert_eq!(epoch.pending_settlements(), 7);
    }

    #[test]
    fn test_settlement_progress_bps() {
        let mut epoch = create_test_epoch(1, 1000, 2000, 4, 2, false);
        assert_eq!(epoch.settlement_progress_bps(), 5000); // 50%

        epoch.num_settled_markets = 4;
        assert_eq!(epoch.settlement_progress_bps(), 10000); // 100%

        epoch.num_settled_markets = 0;
        assert_eq!(epoch.settlement_progress_bps(), 0); // 0%
    }

    #[test]
    fn test_settlement_progress_bps_no_markets() {
        let epoch = create_test_epoch(1, 1000, 2000, 0, 0, false);
        assert_eq!(epoch.settlement_progress_bps(), 10000); // Fully settled if no markets
    }

    #[test]
    fn test_time_until_start() {
        let epoch = create_test_epoch(1, 1000, 2000, 5, 0, false);
        assert_eq!(epoch.time_until_start(500), 500);
        assert_eq!(epoch.time_until_start(1000), 0);
        assert_eq!(epoch.time_until_start(1500), -500);
    }

    #[test]
    fn test_time_until_end() {
        let epoch = create_test_epoch(1, 1000, 2000, 5, 0, false);
        assert_eq!(epoch.time_until_end(1500), 500);
        assert_eq!(epoch.time_until_end(2000), 0);
        assert_eq!(epoch.time_until_end(2500), -500);
    }
}
