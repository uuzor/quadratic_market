# Odds Seeding Architecture Analysis

## Executive Summary

This document analyzes the smart contract architecture for odds seeding (LP and market initialization) and settlements in the Quadratic Market protocol. We evaluate the current design, identify potential issues, and propose improvements for better UX and security.

---

## 1. Current Architecture Overview

### 1.1 Market Creation Flow

```
Operator/Bot
    │
    ├─► create_market_group()
    │       └─► MarketGroup PDA created
    │
    ├─► create_market()
    │       ├─► Market PDA created
    │       ├─► q_values initialized (from external odds or zero)
    │       ├─► status = PreOpen
    │       └─► Linked to current epoch
    │
    ├─► add_market_to_group()
    │       └─► Market linked to group
    │
    └─► activate_seeded_market()
            └─► q_values computed from seed volumes
```

### 1.2 Settlement Flow

```
Match Ends
    │
    ├─► suspend_market() [if not already suspended]
    │
    ├─► [RESULT_DELAY_SECONDS passes]
    │
    ├─► propose_result() [oracle signature]
    │       ├─► status = Proposed
    │       └─► dispute.challenge_deadline set
    │
    ├─► [CHALLENGE_WINDOW_SECONDS passes]
    │
    ├─► finalize_result()
    │       ├─► status = Settled
    │       └─► winning_outcome set
    │
    └─► Users claim payouts via claim_payout()
```

### 1.3 Key Components

| Component | Purpose |
|-----------|---------|
| `Market` | Individual market with q_values, status, backing |
| `MarketGroup` | Groups related markets (same fixture) |
| `Epoch` | Tracks settlement for LP withdrawal gating |
| `GlobalConfig` | Protocol-wide settings |

---

## 2. Current Odds Seeding Implementation

### 2.1 Initial Q-Values on Market Creation

The `create_market` instruction accepts optional `initial_q_values` from external odds sources:

```rust
pub fn create_market_handler(
    ctx: Context<CreateMarket>,
    start_time: i64,
    num_outcomes: u8,
    title: String,
    description: String,
    category: u8,
    lmsr_b_override: Option<u64>,
    initial_q_values: Option<Vec<u64>>,  // ← External odds input
    market_mode: MarketMode,
) -> Result<()> {
    // Validation: prices must be in [1%, 99%]
    if let Some(q_vals) = initial_q_values {
        for outcome_id in 0..num_outcomes {
            let price = lmsr::lmsr_price(&q_array, num_outcomes, outcome_id, lmsr_b)?;
            require!(price >= min_price && price <= max_price, InvalidAmount);
        }
    }
    // Store provided q_values
    market.q_values = q_values;
}
```

### 2.2 Seed-Based Q-Values (activate_seeded_market)

```rust
pub(crate) fn compute_seed_lmsr_q_values(
    seed_positions: &[SeedPosition],
    num_seed_positions: u8,
    market_index: u8,
    num_outcomes: u8,
    total_seed_volume: u64,
    b_fp: u64,
) -> Result<[u64; MAX_OUTCOMES]> {
    // Compute probabilities from seed volumes
    for outcome_id in 0..num_outcomes as usize {
        let probability_fp = (side_volumes[outcome_id] * SCALE) / total_seed_volume;
        let ln_probability = ln_q32(probability_fp)?;
        ln_probabilities[outcome_id] = ln_probability;
    }
    
    // Normalize to get q_values
    for outcome_id in 0..num_outcomes as usize {
        let ln_delta = ln_probabilities[outcome_id] - min_ln;
        q_values[outcome_id] = (b_raw * ln_delta) / SCALE;
    }
}
```

---

## 3. Issues and Analysis

### 3.1 Architecture Strengths

1. **Dual Seeding Mechanisms**
   - Initial q_values from external odds at creation
   - Seed positions can override/update q_values later
   - Flexibility for different market types

2. **LMSR Validation**
   - Bounds checking on prices: [1%, 99%]
   - Prevents extreme odds that could enable free arbitration

3. **Per-Market Backing**
   - `market.backing` tracks market-specific bankroll
   - Seed capital + bet costs collected
   - Separates LP pool liability from market liability

4. **Epoch-Based Settlement Tracking**
   - LP withdrawals gated by epoch settlement
   - Prevents LP exit before markets resolve

### 3.2 Issues and Risks

#### Issue 1: No Automatic Market Opening After Seeding

**Problem:** Markets are created in `PreOpen` status but there's no automatic transition to `Open` after seeding completes.

**Current Flow:**
```
create_market() → status = PreOpen
init_outcome_mint() → (no status change)
activate_seeded_market() → (no status change) ❌
```

**Impact:** Markets remain in `PreOpen` indefinitely, users cannot trade.

**Recommendation:** Add automatic transition to `Open` after:
- All outcome mints are initialized AND
- Seed requirements are met (if seeding is required)

#### Issue 2: Seed Fee Reward Calculation Can Result in Zero Rewards

**Problem:** In `claim_seed_fee_reward_handler`:
```rust
let reward = ((market_mut.seed_fee_pool as u128)
    .checked_mul(seed.amount as u128)
    / unclaimed_losing_seed_total as u128) as u64;
require!(reward > 0, QuadraticMarketError::InvalidAmount);  // ← Fails
```

**Impact:** Dust amounts can fail due to integer truncation.

**Recommendation:** 
- Use `saturating_sub` instead of failing on small rewards
- Or set a minimum claimable threshold (e.g., 1000 lamports)

#### Issue 3: Settlement Timing is Operator-Dependent

**Problem:** Settlement requires the bot to call multiple instructions with proper timing.

**Impact:** If bot is down, markets don't settle automatically.

**Recommendation:** 
- Make `finalize_result` permissionless (already done ✅)
- Consider auto-proposing based on external oracle event
- Add emergency admin override that can propose + finalize in one tx

#### Issue 4: Complex Market Group State

**Problem:** `MarketGroup` tracks multiple complex data structures:
- `seed_positions[]` - up to 16 seeds
- `seed_fee_pools[]` - per market
- `state_probabilities[]` - for same-game markets
- `outcome_state_masks[]` - complex correlation logic

**Recommendation:**
- Consider separating correlation logic into a separate module
- Document the state machine more clearly
- Add invariant checks in tests

#### Issue 5: Q-Value Interpretation Confusion

**Problem:** The contract stores `q_values` directly, but sources vary:
- Initial q_values from external odds (pre-seed)
- Seed-based q_values computed from seed volumes
- Both stored in the same field

**Impact:** Bot needs to track which source was used.

**Recommendation:** Add metadata to track q_value evolution:
```rust
pub enum QValueSource {
    ExternalOdds,   // From API, before any trading
    SeedDerived,    // Computed from seed positions
    MarketDerived   // Changed by user trading
}
```

#### Issue 6: No Price Bounds on Seed-Derived Q-Values

**Problem:** When `activate_seeded_market()` computes q_values from seed volumes, no validation that resulting prices are reasonable.

**Example:** If someone seeds 99% on outcome A and 1% on outcome B, prices become extreme.

**Recommendation:** Add bounds check in `compute_seed_lmsr_q_values`:
```rust
let price = lmsr_price(&q_values, num_outcomes, outcome_id, b)?;
require!(price >= MIN_PRICE && price <= MAX_PRICE, InvalidPriceBounds);
```

---

## 4. UX Improvements

### 4.1 Automated Market Lifecycle

**Current:**
```
Manual bot calls → suspend_market() → wait → propose_result() → wait → finalize_result()
```

**Proposed:** Cron-triggered or event-driven settlement

### 4.2 Improved Settlement Status Visibility

**Current:** Users must check `market.status` and `dispute` account.

**Proposed:** Add settlement events:
```rust
#[event]
pub struct MarketSettled {
    pub market_id: u64,
    pub winning_outcome: u8,
    pub settle_price: u64,
    pub total_payout: u64,
}
```

### 4.3 Seed Position Discovery

**Current:** Bot must track seed positions in external state.

**Proposed:** Index seed positions by market in the Market account.

---

## 5. Recommended Architecture Changes

### 5.1 Separation of Concerns

```
┌─────────────────────────────────────────────────────────────┐
│                     Market Factory                          │
│  - create_market_with_odds()                               │
│  - Validates odds bounds                                   │
│  - Sets initial q_values                                   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    Seeding Module                           │
│  - activate_seeded_market()                                │
│  - compute_seed_lmsr_q_values()                           │
│  - Validates seed requirements                             │
│  - Auto-transitions to Open                               │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                   Trading Module                            │
│  - buy_shares()                                           │
│  - sell_shares()                                          │
│  - Updates q_values                                       │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                   Settlement Module                         │
│  - propose_result() / admin_override()                   │
│  - finalize_result()                                      │
│  - claim_payout()                                        │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Settlement Timing Configuration

```rust
pub struct SettlementConfig {
    pub delay_seconds: i64,           // After match start
    pub challenge_window_seconds: i64, // Dispute period
    pub auto_finalize: bool,          // Permissionless finalize
    pub max_settlement_delay: i64,    // Auto-void deadline
}
```

### 5.3 Odds Source Tracking

```rust
pub struct Market {
    // ... existing fields ...
    
    // Odds metadata
    pub odds_source: Pubkey,           // Oracle that provided odds
    pub odds_timestamp: i64,            // When odds were fetched
    pub odds_confidence: u8,            // 0-100 confidence score
}
```

---

## 6. Bot Integration Recommendations

### 6.1 TXOdds Integration

TXOdds provides **de-marginated odds** which are ideal for seeding:

```python
# bot/txodds.py already supports this
odds = await txodds.get_odds_snapshot(fixture_id)
de_marginated_odds = odds[0].odds_demarginated  # True probabilities

# Convert to q_values
q_values = compute_q_values_from_prices(de_marginated_odds)
```

**Benefits:**
- True implied probabilities without bookmaker vig
- Better price discovery
- Reduced LP adverse selection

### 6.2 Market Timing Configuration

```python
# bot/config.py - Recommended timing config

# For football (90 min + extra time):
RESULT_DELAY_SECONDS = 5400  # 90 min match + 30 min buffer

# For US sports:
# NFL: 7200 (2+ hours including OT)
# NBA: 5400 (2 hours + OT)

# Challenge window (dispute period):
CHALLENGE_WINDOW_SECONDS = 60  # Short for fast markets
```

### 6.3 Settlement Check Flow

```python
async def check_and_settle_market(chain, market_id, api):
    market = await chain.fetch_market(market_id)
    
    # 1. Check market is past start time
    now = time.time()
    if now < market.start_time:
        return "NOT_STARTED"
    
    # 2. Check market is suspended
    if market.status == "Open":
        await chain.suspend_market(market_id)
        return "SUSPENDED"
    
    # 3. Check delay passed
    if now < market.start_time + RESULT_DELAY_SECONDS:
        return "WAITING_DELAY"
    
    # 4. Fetch result from API
    result = await api.get_score(fixture_id)
    if not result:
        return "NO_RESULT"
    
    # 5. Propose result
    await chain.admin_override(market_id, result.winning_outcome)
    
    return "SETTLED"
```

---

## 7. Security Considerations

### 7.1 Price Manipulation Prevention

**Current:** Bounds check at [1%, 99%]

**Enhancements:**
```rust
require!(is_valid_odds_spread(&q_values), InvalidOddsSpread);
require!(is_valid_seed_distribution(&seed_positions), InvalidSeedDistribution);
```

### 7.2 Oracle Security

**Current:** Single oracle key in `GlobalConfig`

**Recommendation:**
- Support multiple oracle sources
- Require M-of-N signatures for settlement
- Add oracle accountability for incorrect results

### 7.3 Front-Running Prevention

**Recommendation:** Consider:
- Private transactions for large trades
- TWAP/VAMM pricing for large orders
- Commit-reveal scheme for odds submission

---

## 8. Testing Recommendations

### 8.1 Fuzz Testing for Q-Value Computation

```rust
#[test]
fn fuzz_seed_q_values() {
    for _ in 0..1000 {
        let seeds = random_seed_positions();
        let result = compute_seed_lmsr_q_values(...);
        assert!(result.is_ok());
        
        // Verify price bounds
        let prices = compute_prices(&result);
        for price in prices {
            assert!(price >= MIN_PRICE && price <= MAX_PRICE);
        }
    }
}
```

### 8.2 Settlement Edge Cases

```rust
#[test]
fn test_settlement_edge_cases() {
    // Case 1: No bets, only seeds
    // Case 2: All bets on winning outcome
    // Case 3: All bets on losing outcome
    // Case 4: Tie/draw in 2-way market (should fail)
    // Case 5: Market voided mid-bet
    // Case 6: Oracle proposes after deadline
}
```

---

## 9. Summary of Recommendations

### High Priority

| # | Recommendation | Impact |
|---|---------------|--------|
| 1 | Auto-transition to Open after seeding | Enables trading |
| 2 | Add bounds check for seed-derived q_values | Prevents manipulation |
| 3 | Make finalize_result more robust | Faster settlement |
| 4 | Add settlement events for UX | Better UX |

### Medium Priority

| # | Recommendation | Impact |
|---|---------------|--------|
| 5 | Track q_value source evolution | Better debugging |
| 6 | Separate correlation module | Cleaner code |
| 7 | Multi-oracle support | Security |
| 8 | Seed position indexing | Easier tracking |

### Low Priority

| # | Recommendation | Impact |
|---|---------------|--------|
| 9 | TWAP for large trades | MEV protection |
| 10 | Oracle accountability | Accountability |

---

## 10. Conclusion

The current architecture provides a solid foundation for odds seeding and settlements, with good separation of concerns and appropriate validation. The main areas for improvement are:

1. **UX**: Automated market lifecycle transitions and better status visibility
2. **Security**: Additional bounds checking on seed-derived prices
3. **Reliability**: More robust settlement mechanisms
4. **Debugging**: Better tracking of q_value evolution

The TXOdds integration in the bot is well-designed for fetching accurate de-marginated odds, which will improve LP profitability and price discovery.
