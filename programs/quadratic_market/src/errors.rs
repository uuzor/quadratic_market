use anchor_lang::prelude::*;

#[error_code]
pub enum QuadraticMarketError {
    #[msg("Not authorized")]
    Unauthorized = 0,

    #[msg("Protocol is paused")]
    Paused = 1,

    #[msg("Invalid amount")]
    InvalidAmount = 2,

    #[msg("Amount too small")]
    AmountTooSmall = 3,

    #[msg("Insufficient liquidity")]
    InsufficientLiquidity = 4,

    #[msg("Math overflow")]
    MathOverflow = 5,

    #[msg("Math underflow")]
    MathUnderflow = 6,

    #[msg("Market backing insufficient to cover position liability")]
    InsufficientMarketBacking = 7,

    // 100-199: Market errors
    #[msg("Market not open for trading")]
    MarketNotOpen = 100,

    #[msg("Market has already started")]
    MarketAlreadyStarted = 101,

    #[msg("Invalid outcome ID")]
    InvalidOutcomeId = 102,

    #[msg("Maximum exposure reached")]
    MaxExposureReached = 103,

    #[msg("Market already settled")]
    MarketAlreadySettled = 104,

    #[msg("Invalid number of outcomes")]
    InvalidNumOutcomes = 105,

    #[msg("Market not settled")]
    MarketNotSettled = 106,

    #[msg("Market not voidable")]
    MarketNotVoidable = 108,

    #[msg("Invalid market status for this operation")]
    InvalidMarketStatus = 109,

    #[msg("Market has expired for new positions")]
    MarketExpired = 110,

    #[msg("Market settlement deadline has not passed")]
    SettlementDeadlineNotPassed = 111,

    // 200-299: Trading errors
    #[msg("Insufficient shares to sell")]
    InsufficientShares = 200,

    #[msg("LMSR cost exceeds maximum payment")]
    LmsrCostExceedsMax = 201,

    #[msg("LMSR sell price below minimum")]
    LmsrSellBelowMin = 202,

    #[msg("Bet size exceeds maximum allowed")]
    BetTooLarge = 203,

    #[msg("Outcome probability is below the minimum floor — odds too short")]
    OddsFloor = 204,

    #[msg("Outcome token mint does not match expected outcome")]
    WrongOutcomeToken = 205,

    #[msg("Payout has already been claimed")]
    PayoutAlreadyClaimed = 206,

    #[msg("There are no winning positions to settle")]
    NoWinningPositions = 207,

    #[msg("Insufficient free liquidity for this operation")]
    InsufficientFreeLiquidity = 208,

    #[msg("Insufficient LP shares")]
    InsufficientLpShares = 209,

    #[msg("Shares are still locked")]
    SharesStillLocked = 210,

    #[msg("Challenge window has expired")]
    ChallengeWindowExpired = 211,

    // 300-399: Settlement errors
    #[msg("Challenge window still active")]
    ChallengeWindowActive = 300,

    #[msg("No dispute to finalize")]
    NoDisputeToFinalize = 301,

    #[msg("Invalid winning outcome")]
    InvalidWinningOutcome = 302,

    #[msg("Invalid proposed outcome")]
    InvalidProposedOutcome = 303,

    #[msg("Invalid oracle signature")]
    InvalidOracleSignature = 304,

    // 400-499: LP/Epoch errors
    #[msg("Epoch is paused — no deposits or withdrawals allowed")]
    EpochPaused = 400,

    #[msg("Epoch is not complete")]
    EpochNotComplete = 401,

    #[msg("Cooldown has not elapsed")]
    CooldownNotElapsed = 402,

    #[msg("Epoch account mismatch")]
    EpochAccountMismatch = 403,

    #[msg("Epoch withdrawals are not enabled")]
    EpochWithdrawalsNotEnabled = 404,

    #[msg("No pending liquidity found")]
    NoPendingLiquidity = 405,

    // 500-599: Correlation errors
    #[msg("Market group not found")]
    MarketGroupNotFound = 500,

    #[msg("Market group is full")]
    MarketGroupFull = 501,

    #[msg("Market not in group")]
    MarketNotInGroup = 502,

    #[msg("Market already in group")]
    MarketAlreadyInGroup = 503,

    #[msg("Correlation out of bounds")]
    CorrelationOutOfBounds = 504,

    // 600-699: Risk management errors
    #[msg("Group exposure exceeded")]
    GroupExposureExceeded = 600,

    #[msg("Market group event has started")]
    GroupEventStarted = 601,

    #[msg("Correlation matrix is locked after first trade")]
    CorrelationMatrixLocked = 602,

    #[msg("Invalid account in remaining_accounts")]
    InvalidRemainingAccount = 603,

    #[msg("Correlation calculation overflow")]
    CorrelationOverflow = 604,

    #[msg("Operator list is full")]
    OperatorListFull = 605,

    #[msg("Operator not found")]
    OperatorNotFound = 606,

    #[msg("Direct share trading is disabled on fixed-odds markets")]
    DirectTradingDisabled = 607,

    #[msg("Order is not in a cancellable state")]
    OrderNotCancellable = 608,

    #[msg("Order has expired")]
    OrderExpired = 609,

    #[msg("Order has not expired")]
    OrderNotExpired = 610,

    #[msg("Order is not open for filling")]
    OrderNotFillable = 611,

    #[msg("Fill amount exceeds remaining order quantity")]
    FillExceedsOrder = 612,

    #[msg("Seeded market has not reached minimum bootstrapping requirements")]
    SeedMarketNotReady = 613,

    #[msg("Swap below minimum threshold")]
    SwapBelowMinimum = 614,

    // 900-999: Misc
    #[msg("Not implemented")]
    NotImplemented = 900,
}
