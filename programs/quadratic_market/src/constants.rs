// Fixed-point arithmetic constants for basis points
pub const BPS_SCALE: u64 = 10_000; // Basis points scale (10000 = 1.0x)

// Q32.32 fixed-point arithmetic constants (still used by some modules)
pub const SCALE: u64 = 1_u64 << 32; // 4_294_967_296
pub const LN2_FP: i64 = 2_973_032_047; // Precomputed ln(2) in Q32.32

// Max outcomes per market
pub const MAX_OUTCOMES: usize = 8;

// ERC4626 inflation fix
pub const MIN_FIRST_LIQUIDITY: u64 = 1000; // Lock 1000 base units on first deposit

// Settlement — sports-focused: short window, oracle-driven
pub const DEFAULT_CHALLENGE_WINDOW: i64 = 300; // 5 minutes
pub const DEFAULT_SETTLEMENT_DEADLINE: i64 = 14_400; // 4 hours after start_time

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
    pub const WITHDRAWAL: &[u8] = b"withdrawal";
    pub const MARKET_GROUP: &[u8] = b"market_group";
    pub const BET_SLIP: &[u8] = b"bet_slip";
    pub const PENDING: &[u8] = b"pending";
    pub const ORDER: &[u8] = b"order";
    pub const EPOCH: &[u8] = b"epoch";
    pub const EPOCH_VAULT: &[u8] = b"epoch_vault";
    pub const SLIP: &[u8] = b"slip";
}

// Market groups (tracking only, no correlation)
pub const MAX_GROUP_MARKETS: usize = 3; // 1X2, O/U, GG/NG

// Bet slip - maximum 5 legs to avoid BPF stack overflow
pub const MAX_SLIP_LEGS: usize = 5;

// LP epoch / timing
pub const DEFAULT_EPOCH_DURATION_SECONDS: i64 = 86_400; // 24 hours
pub const DEFAULT_WITHDRAWAL_COOLDOWN_SECONDS: i64 = 86_400; // 24 hours

// Sports risk controls
pub const DEFAULT_MAX_SINGLE_BET: u64 = 10_000_000_000; // 10,000 USDC
pub const DEFAULT_MIN_ODDS_BPS: u64 = 100; // 1% minimum (1.01x)
pub const DEFAULT_MAX_ODDS_BPS: u64 = 100_000; // 10x maximum
pub const DEFAULT_HOUSE_FEE_BPS: u64 = 500; // 5% house fee

// Operator allowlist
pub const MAX_OPERATORS: usize = 8;
