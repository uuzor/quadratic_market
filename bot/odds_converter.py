"""
Odds conversion utilities for the Quadratic Market sports bot.

Converts decimal odds from The-Odds-API or TxODDS to LMSR q_values.
The LMSR pricing engine uses q_values to determine implied probabilities.

LMSR Price Formula (Q32.32):
    price_i = exp(q_i / B) / sum(exp(q_j / B))

This module provides:
1. Decimal odds to q_values conversion
2. TxODDS odds to q_values conversion (de-marginated)
3. Q-value validation against on-chain bounds (1%-99%)

Use de-marginated odds from TxODDS to avoid double-margining:
    - Bookmaker vig is already included in marginated odds
    - Your buy_fee_bps adds another margin
    - De-marginated = true probability without vig
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Optional

SCALE = 1 << 32  # Q32.32 fixed point scale
DEFAULT_B = 100_000_000  # 100 USDC liquidity parameter

# Bounds for valid prices (1% to 99%)
MIN_PRICE_BPS = 100  # 1%
MAX_PRICE_BPS = 9900  # 99%


def _exp_neg_fp(x: int) -> int:
    """
    Approximation for exp(-x) where x is in Q32.32.
    Uses Taylor series: exp(-x) ≈ 1 - x + x²/2 - x³/6 + x⁴/24
    """
    if x == 0:
        return SCALE
    
    max_exp = SCALE * 10
    if x > max_exp:
        return 0
    
    x_abs = x
    x_sq = (x_abs * x_abs) // SCALE
    x_cu = (x_sq * x_abs) // SCALE
    x_4 = (x_cu * x_abs) // SCALE
    
    result = SCALE
    result -= x_abs
    result += x_sq // 2
    result -= x_cu // 6
    result += x_4 // 24
    
    return max(0, result)


def lmsr_price_fp(q_values: list[int], b: int, outcome_idx: int) -> int:
    """
    Compute LMSR price for an outcome in Q32.32.
    
    Args:
        q_values: List of q values
        b: LMSR B parameter
        outcome_idx: Index of outcome to price
        
    Returns:
        Price in Q32.32 (between 0 and SCALE)
    """
    if not q_values or b <= 0:
        raise ValueError("Invalid parameters")
    
    n = len(q_values)
    max_q = max(q_values)
    
    exps = []
    for q in q_values:
        if q == max_q:
            exps.append(SCALE)
        else:
            diff = max_q - q
            exponent = (diff * SCALE) // b
            exps.append(_exp_neg_fp(exponent))
    
    sum_exp = sum(exps)
    if sum_exp == 0:
        return SCALE // n
    
    return (exps[outcome_idx] * SCALE) // sum_exp


def decimal_odds_to_q_values(
    odds: list[float],
    b: int = DEFAULT_B,
) -> list[int]:
    """
    Convert decimal odds to LMSR q_values using proper inverse calculation.
    
    For 2 outcomes: q_0 = B * ln(price_0 / (1 - price_0))
    For N outcomes: Uses gradient descent to find q_values that match target prices.
    
    Args:
        odds: List of decimal odds (e.g., [2.50, 3.20, 2.80] for Home/Draw/Away)
        b: Liquidity parameter B (default 100_000_000 = 100 USDC)
        
    Returns:
        List of q_values (integers) for the LMSR pricing engine.
        
    Example:
        >>> odds = [2.50, 3.20, 2.80]
        >>> q = decimal_odds_to_q_values(odds)
        >>> # Produces q_values that give prices matching implied probabilities
    """
    if not odds:
        return []
    
    # Convert decimal odds to implied probabilities
    probabilities = []
    for o in odds:
        if o <= 0:
            probabilities.append(0)
        else:
            probabilities.append(1.0 / o)
    
    # Normalize to sum to 1
    total = sum(probabilities)
    if total <= 0:
        return [0] * len(odds)
    
    normalized = [p / total for p in probabilities]
    
    # Use closed form for 2 outcomes, gradient descent for N
    if len(normalized) == 2:
        return _compute_q_values_2_outcomes(normalized[0], b)
    else:
        return _compute_q_values_gradient(normalized, b)


def _compute_q_values_2_outcomes(price0: float, b: int) -> list[int]:
    """Closed-form solution for 2-outcome market."""
    if price0 <= 0 or price0 >= 1:
        price0 = 0.5  # Default to 50/50
    
    ratio = price0 / (1 - price0)
    ln_ratio = math.log(ratio)
    ln_ratio_fp = int(ln_ratio * SCALE)
    q0 = (b * ln_ratio_fp) // SCALE
    
    return [q0, 0]


def _compute_q_values_gradient(
    target_prices: list[float],
    b: int,
    max_iterations: int = 100,
) -> list[int]:
    """Gradient descent to find q_values matching target prices."""
    n = len(target_prices)
    targets = [int(p * SCALE) for p in target_prices]
    
    q_values = [0] * n
    lr = SCALE // 1000
    
    for _ in range(max_iterations):
        prices = [lmsr_price_fp(q_values, b, i) for i in range(n)]
        
        total_error = sum(abs(prices[i] - targets[i]) for i in range(n))
        if total_error < 1:
            break
        
        for i in range(n):
            error = prices[i] - targets[i]
            delta = (lr * error) // SCALE
            q_values[i] -= delta
            
            max_q = b * 100
            min_q = -b * 100
            q_values[i] = max(min_q, min(max_q, q_values[i]))
    
    return q_values


def q_values_from_api_odds(
    home_odds: float,
    draw_odds: Optional[float] = None,
    away_odds: Optional[float] = None,
) -> list[int]:
    """
    Convert API odds (home, draw, away) to q_values.
    
    Args:
        home_odds: Decimal odds for home win
        draw_odds: Decimal odds for draw (None for 2-way markets)
        away_odds: Decimal odds for away win
        
    Returns:
        List of q_values for [home, draw?, away] outcomes
    """
    odds = [home_odds]
    if draw_odds is not None:
        odds.append(draw_odds)
    if away_odds is not None:
        odds.append(away_odds)
    
    return decimal_odds_to_q_values(odds)


def validate_q_values(q_values: list[int], b: int = DEFAULT_B) -> tuple[bool, list[float], list[str]]:
    """
    Validate that q_values produce reasonable prices (1%-99%).
    
    Returns:
        Tuple of (valid, prices, errors)
    """
    errors = []
    prices = []
    
    n = len(q_values)
    min_price = SCALE // 100
    max_price = (SCALE * 99) // 100
    
    for i in range(n):
        price_fp = lmsr_price_fp(q_values, b, i)
        price = price_fp / SCALE
        prices.append(price)
        
        if price < 0.01:
            errors.append(f"Outcome {i}: price {price:.4f} < 1%")
        if price > 0.99:
            errors.append(f"Outcome {i}: price {price:.4f} > 99%")
    
    total = sum(prices)
    if abs(total - 1) > 0.01:
        errors.append(f"Prices sum to {total:.4f}, expected ~1.0")
    
    return len(errors) == 0, prices, errors


@dataclass
class MarketType:
    """Defines a type of market that can be created per fixture."""
    category: int
    name: str
    key: str  # The-Odds-API market key
    outcomes: list[str]  # Outcome names in order [outcome_0, outcome_1, ...]
    has_draw: bool  # Whether this market type has a draw outcome


# Football market types supported
FOOTBALL_MARKET_TYPES = [
    MarketType(
        category=0,
        name="Match Result",
        key="h2h",
        outcomes=["Home Win", "Draw", "Away Win"],
        has_draw=True,
    ),
    MarketType(
        category=1,
        name="Both Teams To Score",
        key="btts",
        outcomes=["Yes", "No"],
        has_draw=False,
    ),
    MarketType(
        category=2,
        name="Over/Under 2.5 Goals",
        key="totals",
        outcomes=["Over 2.5", "Under 2.5"],
        has_draw=False,
    ),
]


def get_market_type(category: int) -> Optional[MarketType]:
    """Get market type by category number."""
    for mt in FOOTBALL_MARKET_TYPES:
        if mt.category == category:
            return mt
    return None


def get_market_outcome_name(category: int, outcome_id: int) -> str:
    """Get the name of an outcome by category and outcome_id."""
    mt = get_market_type(category)
    if mt and outcome_id < len(mt.outcomes):
        return mt.outcomes[outcome_id]
    return f"Outcome {outcome_id}"