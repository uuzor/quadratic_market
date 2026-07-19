use anchor_lang::prelude::*;

#[error_code]
pub enum QuadraticMarketError {
    // 0-99: General errors
    #[msg("Not authorized")]
    Unauthorized = 0,
    #[msg("Protocol is paused")]
    Paused = 1,
    #[msg("Invalid amount")]
    InvalidAmount = 2,
    #[msg("Insufficient liquidity")]
    InsufficientLiquidity = 3,
    #[msg("Math overflow")]
    MathOverflow = 4,
    #[msg("Math underflow")]
    MathUnderflow = 5,

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
    #[msg("Slippage exceeded: minimum shares not received")]
    SlippageExceeded = 201,
    #[msg("LMSR cost exceeds maximum payment")]
    LmsrCostExceedsMax = 202,
    #[msg("LMSR sell price below minimum")]
    LmsrSellBelowMin = 203,
    #[msg("Bet size exceeds maximum allowed")]
    BetTooLarge = 204,
    #[msg("Outcome probability is below the minimum floor — odds too short")]
    OddsFloor = 205,

    // 300-399: Settlement errors
    #[msg("Challenge window still active")]
    ChallengeWindowActive = 300,
    #[msg("Challenge window has expired")]
    ChallengeWindowExpired = 301,
    #[msg("Invalid proposed outcome")]
    InvalidProposedOutcome = 305,

    // 400-499: LP errors
    #[msg("Amount too small for first deposit")]
    AmountTooSmall = 400,
    #[msg("Insufficient LP shares")]
    InsufficientLpShares = 401,
    #[msg("Withdrawal request already exists")]
    WithdrawalAlreadyExists = 402,
    #[msg("No withdrawal request found")]
    NoWithdrawalRequest = 403,
    #[msg("Insufficient free liquidity for withdrawal")]
    InsufficientFreeLiquidity = 404,
    #[msg("Withdrawal cooldown has not elapsed")]
    CooldownNotElapsed = 405,
    #[msg("No pending liquidity to activate")]
    NoPendingLiquidity = 406,
    #[msg("LP shares are still locked pending activation")]
    SharesStillLocked = 407,

    // 500-599: Claim errors
    #[msg("No winning positions to claim")]
    NoWinningPositions = 500,
    #[msg("Payout already claimed")]
    PayoutAlreadyClaimed = 501,
    #[msg("Wrong outcome token for claim")]
    WrongOutcomeToken = 502,

    // 600-699: Swap errors
    #[msg("Swap amount below minimum")]
    SwapBelowMinimum = 600,
    #[msg("Swap failed")]
    SwapFailed = 601,

    // 700-799: Correlated market / slip errors
    #[msg("Market group not found")]
    MarketGroupNotFound = 700,
    #[msg("Market already belongs to a group")]
    MarketAlreadyInGroup = 701,
    #[msg("Market group is full")]
    MarketGroupFull = 702,
    #[msg("Correlation weight exceeds maximum")]
    CorrelationOutOfBounds = 703,
    #[msg("Group exposure cap exceeded")]
    GroupExposureExceeded = 704,
    #[msg("Market is not in the specified group")]
    MarketNotInGroup = 705,
    #[msg("Bet slip has no legs")]
    SlipNoLegs = 706,
    #[msg("Bet slip has too many legs")]
    SlipTooManyLegs = 707,
    #[msg("Bet slip cost exceeds maximum payment")]
    SlipCostExceeded = 708,
    #[msg("Bet slip not fully settled")]
    SlipNotSettled = 709,
    #[msg("Bet slip already claimed")]
    SlipAlreadyClaimed = 710,
    #[msg("Correlation calculation overflow")]
    CorrelationOverflow = 711,
    #[msg("Market group event has started")]
    GroupEventStarted = 712,
    #[msg("Correlation matrix is locked after first trade")]
    CorrelationMatrixLocked = 713,
    #[msg("Invalid account in remaining_accounts")]
    InvalidRemainingAccount = 714,
    #[msg("Slip lock update failed")]
    SlipLockUpdateFailed = 715,
    #[msg("Bet slip has a voided leg — refunding stake")]
    SlipPartiallyVoided = 716,
    #[msg("Operator list is full")]
    OperatorListFull = 717,
    #[msg("Operator not found")]
    OperatorNotFound = 718,
    #[msg("Direct share trading is disabled on fixed-odds markets")]
    DirectTradingDisabled = 719,
    #[msg("Order is not in a cancellable state")]
    OrderNotCancellable = 720,
    #[msg("Order has not expired")]
    OrderNotExpired = 721,
    #[msg("Order has expired")]
    OrderExpired = 725,
    #[msg("Order is not open for filling")]
    OrderNotFillable = 722,
    #[msg("Fill amount exceeds remaining order quantity")]
    FillExceedsOrder = 723,
    #[msg("Bet slip has already been cashed out")]
    SlipAlreadyCashedOut = 724,
    #[msg("Bet slip has expired")]
    SlipExpired = 726,
    #[msg("Bet slip has not expired yet")]
    SlipNotExpired = 727,

    // 800-899: Epoch errors
    #[msg("Epoch has not completed — not all markets are settled")]
    EpochNotComplete = 800,
    #[msg("Withdrawals are not yet enabled for this epoch")]
    EpochWithdrawalsNotEnabled = 801,
    #[msg("Epoch is paused — no deposits or withdrawals allowed")]
    EpochPaused = 802,
    #[msg("Market does not belong to the specified epoch")]
    MarketEpochMismatch = 803,
    #[msg("Epoch account does not match the market's epoch")]
    EpochAccountMismatch = 804,
    #[msg("No active epoch — call init_epoch first")]
    NoActiveEpoch = 805,
    #[msg("Bet is not refundable — protocol is not paused")]
    NotPaused = 806,

    // 900-999: Settlement errors
    #[msg("Invalid TxLINE fixture ID")]
    InvalidTxlineFixtureId = 916,
    #[msg("TxLINE proof validation failed")]
    TxlineProofValidationFailed = 917,
    #[msg("Legs from same market with different outcomes are mutually exclusive")]
    CorrelatedLegsMutuallyExclusive = 919,
}
