use crate::constants::MAX_GROUP_MARKETS;
use anchor_lang::prelude::*;

/// Correlation score between markets in a group.
/// Stored as basis points (10000 = 1.0 correlation)
/// Formula for payout: multiplier = 1 - (correlation_bps * 25 / 10000)
///
/// Example:
/// - 10000 bps correlation → multiplier = 1 - (10000 * 25 / 10000) = 0.75 (25% discount)
/// - 6000 bps correlation → multiplier = 1 - (6000 * 25 / 10000) = 0.85 (15% discount)
/// - 0 bps correlation → multiplier = 1.0 (no discount)
pub const CORRELATION_BPS_MULTIPLIER: u64 = 25;

/// Market indices for correlation matrix
pub const MARKET_INDEX_1X2: usize = 0;
pub const MARKET_INDEX_OU: usize = 1;
pub const MARKET_INDEX_GGNG: usize = 2;

/// Default correlation matrix for soccer markets (basis points).
/// Same-market outcomes are 0 (mutually exclusive within same market).
/// Cross-market correlations are empirical estimates.
#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct CorrelationMatrix {
    /// Correlation scores as basis points (0-10000).
    /// Stored as upper triangle: [1X2↔OU, 1X2↔GGNG, OU↔GGNG]
    /// Index mapping: [0]=1X2↔OU, [1]=1X2↔GGNG, [2]=OU↔GGNG
    pub correlations: [u16; 3],
}

impl Default for CorrelationMatrix {
    fn default() -> Self {
        // Default soccer correlations (can be updated by admin)
        // Home Win ↔ Over 2.5: 0.6 correlation (60%)
        // Home Win ↔ GG: 0.7 correlation (70%)
        // Over 2.5 ↔ GG: 0.8 correlation (80%)
        Self {
            correlations: [
                6000, // 1X2↔OU: 60%
                7000, // 1X2↔GGNG: 70%
                8000, // OU↔GGNG: 80%
            ],
        }
    }
}

impl CorrelationMatrix {
    /// Get correlation between two market indices.
    /// Returns correlation in basis points (0-10000).
    pub fn get_correlation(&self, idx1: usize, idx2: usize) -> u16 {
        if idx1 == idx2 {
            return 0; // Same market, handled separately
        }

        // Map to upper triangle index
        let (i, j) = if idx1 < idx2 {
            (idx1, idx2)
        } else {
            (idx2, idx1)
        };

        match (i, j) {
            (MARKET_INDEX_1X2, MARKET_INDEX_OU) => self.correlations[0],
            (MARKET_INDEX_1X2, MARKET_INDEX_GGNG) => self.correlations[1],
            (MARKET_INDEX_OU, MARKET_INDEX_GGNG) => self.correlations[2],
            _ => 0, // Unknown combination
        }
    }

    /// Calculate payout multiplier from correlation score.
    /// multiplier = 1 - (correlation_bps * 25 / 10000)
    /// Which gives:
    /// - 100% correlation (10000 bps) → multiplier = 7500 (75%)
    /// - 60% correlation (6000 bps) → multiplier = 8500 (85%)
    /// - 0% correlation (0 bps) → multiplier = 10000 (100%)
    pub fn payout_multiplier(&self, correlation_bps: u16) -> u64 {
        // Formula: 10000 - (correlation_bps * 25 / 100)
        // Using integer math to avoid float
        let discount = (correlation_bps as u64) * CORRELATION_BPS_MULTIPLIER / 100;
        10000u64.saturating_sub(discount)
    }
}

/// MarketGroup with correlation matrix for LP protection.
/// Each market (1X2, O/U, GG/NG) settles independently with its own oracle submission.
/// Correlation matrix reduces bonus/payout for correlated leg combinations.

#[account]
pub struct MarketGroup {
    pub group_id: u64,
    pub creator: Pubkey,
    pub total_group_exposure: u64,
    pub max_group_exposure: u64,
    pub num_markets: u8,
    /// Market IDs in order: 1X2(0), O/U(1), GG/NG(2)
    pub market_ids: [u64; MAX_GROUP_MARKETS],
    pub event_start_time: i64,
    pub title: String,
    pub bump: u8,
    /// Correlation matrix between markets in this group
    pub correlation_matrix: CorrelationMatrix,
}

impl MarketGroup {
    pub const LEN: usize = 8 // discriminator
        + 8   // group_id
        + 32  // creator
        + 8   // total_group_exposure
        + 8   // max_group_exposure
        + 1   // num_markets
        + 24  // market_ids (3 * 8 bytes)
        + 8   // event_start_time
        + (4 + 128) // title
        + 1   // bump
        + 6   // correlation_matrix (3 * u16)
        + 2; // padding to align
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_correlation_matrix() {
        let corr = CorrelationMatrix::default();

        // Check default values
        assert_eq!(corr.correlations[0], 6000); // 1X2↔OU: 60%
        assert_eq!(corr.correlations[1], 7000); // 1X2↔GGNG: 70%
        assert_eq!(corr.correlations[2], 8000); // OU↔GGNG: 80%
    }

    #[test]
    fn test_get_correlation() {
        let corr = CorrelationMatrix::default();

        // Same market = 0
        assert_eq!(corr.get_correlation(0, 0), 0);
        assert_eq!(corr.get_correlation(1, 1), 0);
        assert_eq!(corr.get_correlation(2, 2), 0);

        // Cross-market correlations
        assert_eq!(corr.get_correlation(0, 1), 6000); // 1X2↔OU: 60%
        assert_eq!(corr.get_correlation(1, 0), 6000); // Reverse: 1X2↔OU
        assert_eq!(corr.get_correlation(0, 2), 7000); // 1X2↔GGNG: 70%
        assert_eq!(corr.get_correlation(1, 2), 8000); // OU↔GGNG: 80%
    }

    #[test]
    fn test_payout_multiplier() {
        let corr = CorrelationMatrix::default();

        // Fully correlated (10000 bps = 1.0)
        // multiplier = 1 - (10000 * 25 / 10000) = 1 - 0.25 = 0.75
        let fully_corr_multiplier = corr.payout_multiplier(10000);
        assert_eq!(fully_corr_multiplier, 7500); // 0.75x in bps

        // 60% correlation
        // multiplier = 1 - (6000 * 25 / 10000) = 1 - 0.15 = 0.85
        let sixty_pct_multiplier = corr.payout_multiplier(6000);
        assert_eq!(sixty_pct_multiplier, 8500); // 0.85x in bps

        // Independent (0 bps = 0.0)
        // multiplier = 1 - 0 = 1.0
        let independent_multiplier = corr.payout_multiplier(0);
        assert_eq!(independent_multiplier, 10000); // 1.0x in bps
    }

    #[test]
    fn test_payout_multiplier_formula() {
        // Verify formula: multiplier = 1 - (correlation_bps * 25 / 10000)
        // Stored as BPS: 10000 - correlation_bps * 25

        let corr = CorrelationMatrix::default();

        // 100% correlation: 10000 - 10000 * 0.25 = 7500 (75%)
        assert_eq!(corr.payout_multiplier(10000), 7500);

        // 50% correlation: 10000 - 5000 * 0.25 = 8750 (87.5%)
        assert_eq!(corr.payout_multiplier(5000), 8750);

        // 0% correlation: 10000 - 0 = 10000 (100%)
        assert_eq!(corr.payout_multiplier(0), 10000);
    }
}
