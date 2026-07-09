// Q32.32 fixed-point arithmetic constants
pub const SCALE: u64 = 1_u64 << 32; // 4_294_967_296
pub const ONE_FP: u64 = SCALE; // 1.0 in Q32.32

// Precomputed ln(2) in Q32.32: 0.69314718056 * 2^32
pub const LN2_FP: i64 = 2_973_032_047;

// LMSR defaults
pub const MAX_OUTCOMES: usize = 8;
pub const DEFAULT_LMSR_B: u64 = 100_000_000_000; // 100,000 USDC (6-decimal lamports)
                                                 // lmsr_b is stored as raw 6-decimal lamports (NOT Q32.32). The LMSR math only
                                                 // ever uses B_raw, so storing the raw value lets B span the full u64 range
                                                 // instead of being capped at ~4,294 USDC by a B_raw * SCALE packing.
pub const DEFAULT_LMSR_B_FP: u64 = DEFAULT_LMSR_B; // raw lamports (B_raw)

// ERC4626 inflation fix
pub const MIN_FIRST_LIQUIDITY: u64 = 1000; // Lock 1000 base units on first deposit

// Settlement — sports-focused: short window, oracle-driven
pub const DEFAULT_CHALLENGE_WINDOW: i64 = 300; // 5 minutes
pub const DEFAULT_SETTLEMENT_DEADLINE: i64 = 14_400; // 4 hours after start_time
pub const MAX_DISPUTE_ROUNDS: u32 = 1; // single-round only

// Token
pub const BASE_MINT_DECIMALS: u8 = 6;

// Strings
pub const MAX_TITLE_LEN: usize = 128;
pub const MAX_DESCRIPTION_LEN: usize = 256;

// PDA seeds
pub mod seeds {
    pub const GLOBAL_CONFIG: &[u8] = b"global_config";
    pub const TREASURY: &[u8] = b"treasury";
    pub const LP_MINT: &[u8] = b"lp_mint";
    pub const MARKET: &[u8] = b"market";
    pub const OUTCOME_MINT: &[u8] = b"outcome_mint";
    pub const DISPUTE: &[u8] = b"dispute";
    pub const WITHDRAWAL: &[u8] = b"withdrawal";
    pub const MARKET_GROUP: &[u8] = b"market_group";
    pub const PENDING: &[u8] = b"pending";
    pub const ORDER: &[u8] = b"order";
    pub const EPOCH: &[u8] = b"epoch";
}

// Correlated markets
pub const MAX_GROUP_MARKETS: usize = MAX_OUTCOMES;
pub const MAX_CORRELATION_PAIRS: usize = 16;
pub const MAX_SAME_GAME_STATES: usize = 64;
pub const MAX_SEED_POSITIONS: usize = 16;
pub const CORRELATION_MAX_BPS: u64 = 10_000;
pub const DEFAULT_SEED_FEE_SHARE_BPS: u64 = 500; // 5% of house fees
pub const DEFAULT_MIN_SEED_VOLUME: u64 = 5_000_000_000; // 5,000 USDC (6 decimals)
pub const DEFAULT_MAX_SEED_SIDE_SHARE_BPS: u64 = 7_000; // largest seeded side <= 70%
                                                        // Each outcome must be seeded with at least this much real capital before a
                                                        // PreOpen market can be activated. Seeders are early bettors whose escrowed
                                                        // capital backs the market's bounded LMSR max-loss (B·ln(N)).
pub const MIN_SEED_PER_OUTCOME: u64 = 500_000_000; // 500 USDC (6 decimals)

// LP epoch / timing
pub const DEFAULT_EPOCH_DURATION_SECONDS: i64 = 86_400; // 24 hours
pub const DEFAULT_WITHDRAWAL_COOLDOWN_SECONDS: i64 = 86_400; // 24 hours

// Sports risk controls
pub const DEFAULT_MAX_SINGLE_BET: u64 = 100_000_000_000; // 100,000 USDC — overridable per market
pub const DEFAULT_MIN_OUTCOME_PRICE_BPS: u64 = 100; // 1% minimum implied probability
pub const DEFAULT_BUY_FEE_BPS: u64 = 100; // 1% house fee on direct buys

// Operator allowlist
pub const MAX_OPERATORS: usize = 8;

// Cash-out
pub const DEFAULT_CASH_OUT_MARGIN_BPS: u64 = 500; // 5%
