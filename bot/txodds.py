"""
TxODDS API Client for QuadraticMarket Oracle Integration

Provides access to StablePrice consensus odds data for seeding
prediction markets with near-fair-value prices.

Authentication:
  - Free tier: Guest session (rate limited)
  - Paid tier: API token via /api/token/activate

Key concepts:
  - De-marginated odds: True implied probability (vig stripped out)
  - Marginated odds: Includes bookmaker overround
  - Use de-marginated odds for seeding to avoid double-margining

API Docs: https://txline.txodds.com/documentation
"""

from __future__ import annotations

import httpx
import math
import time
from dataclasses import dataclass, field
from typing import Optional, Any


BASE_URL = "https://txline.txodds.com"


@dataclass
class Fixture:
    """Fixture data from TxODDS."""
    id: int
    start_time: int  # Unix timestamp
    participant1: str  # Home team
    participant2: str  # Away team
    participant1_is_home: int  # 1 = home, 0 = away
    league: str
    sport: str
    status: str  # SCHEDULED, LIVE, FINISHED

    @classmethod
    def from_dict(cls, data: dict) -> "Fixture":
        return cls(
            id=data.get("id", 0),
            start_time=data.get("startTime", 0),
            participant1=data.get("participant1", ""),
            participant2=data.get("participant2", ""),
            participant1_is_home=data.get("participant1IsHome", 1),
            league=data.get("league", ""),
            sport=data.get("sport", ""),
            status=data.get("status", "SCHEDULED"),
        )


@dataclass
class OddsEntry:
    """Odds entry from TxODDS StablePrice."""
    fixture_id: int
    bookmaker: str
    market_type: str  # e.g., "1X2", "OVER_UNDER"
    selection: str  # e.g., "Home", "Draw", "Away"
    odds_marginated: float  # Includes bookmaker vig
    odds_demarginated: float  # True probability (vig removed)
    timestamp: int

    @classmethod
    def from_dict(cls, data: dict) -> "OddsEntry":
        return cls(
            fixture_id=data.get("fixtureId", 0),
            bookmaker=data.get("bookmaker", ""),
            market_type=data.get("marketType", ""),
            selection=data.get("selection", ""),
            odds_marginated=data.get("oddsMarginated", 0.0),
            odds_demarginated=data.get("oddsDemarginated", 0.0),
            timestamp=data.get("timestamp", 0),
        )


@dataclass
class ScoreEntry:
    """Score entry from TxODDS."""
    fixture_id: int
    score_type: str  # FULL_TIME, FIRST_HALF, etc.
    home_score: int
    away_score: int
    timestamp: int

    @classmethod
    def from_dict(cls, data: dict) -> "ScoreEntry":
        return cls(
            fixture_id=data.get("fixtureId", 0),
            score_type=data.get("scoreType", ""),
            home_score=data.get("homeScore", 0),
            away_score=data.get("awayScore", 0),
            timestamp=data.get("timestamp", 0),
        )


class TxODDSClient:
    """
    TxODDS API client for TxLINE.
    
    Usage:
        client = TxODDSClient()
        
        # Free tier (guest session)
        await client.start_guest_session()
        
        # Or paid tier
        await client.activate_api_token("your-api-token")
        
        fixtures = await client.get_fixtures()
        odds = await client.get_odds_snapshot(fixture_id)
    """
    
    def __init__(self, base_url: str = BASE_URL, timeout: float = 15.0):
        self.base_url = base_url
        self.http = httpx.AsyncClient(base_url=base_url, timeout=timeout)
        self.jwt: Optional[str] = None
        self.api_token: Optional[str] = None
    
    async def close(self) -> None:
        """Close the HTTP client."""
        await self.http.aclose()
    
    async def start_guest_session(self) -> str:
        """
        Start a guest session for free tier access.
        Guest sessions are rate-limited.
        
        Returns:
            JWT token for authentication
        """
        response = await self.http.post("/auth/guest/start")
        response.raise_for_status()
        data = response.json()
        self.jwt = data.get("jwt", "")
        return self.jwt
    
    async def activate_api_token(self, api_token: str) -> None:
        """
        Activate an API token for paid tier access.
        
        Args:
            api_token: Your TxLINE API token
        """
        self.api_token = api_token
        response = await self.http.post(
            "/api/token/activate",
            headers={"X-Api-Token": api_token}
        )
        response.raise_for_status()
        data = response.json()
        self.jwt = data.get("jwt", "")
    
    def _get_auth_headers(self) -> dict[str, str]:
        """Get authentication headers."""
        headers = {}
        if self.jwt:
            headers["Authorization"] = f"Bearer {self.jwt}"
        if self.api_token:
            headers["X-Api-Token"] = self.api_token
        return headers
    
    async def get_fixtures(self) -> list[Fixture]:
        """
        Fetch all fixtures (upcoming and current matches).
        
        Returns:
            List of Fixture objects
        """
        response = await self.http.get(
            "/api/fixtures/snapshot",
            headers=self._get_auth_headers()
        )
        response.raise_for_status()
        data = response.json()
        return [Fixture.from_dict(f) for f in data]
    
    async def get_odds_snapshot(self, fixture_id: int) -> list[OddsEntry]:
        """
        Fetch odds snapshot for a specific fixture.
        
        Args:
            fixture_id: The fixture ID to fetch odds for
            
        Returns:
            List of OddsEntry objects
        """
        response = await self.http.get(
            f"/api/odds/snapshot/{fixture_id}",
            headers=self._get_auth_headers()
        )
        response.raise_for_status()
        data = response.json()
        return [OddsEntry.from_dict(o) for o in data]
    
    async def get_scores_snapshot(self, fixture_id: int) -> list[ScoreEntry]:
        """
        Fetch scores snapshot for a specific fixture.
        
        Args:
            fixture_id: The fixture ID to fetch scores for
            
        Returns:
            List of ScoreEntry objects
        """
        response = await self.http.get(
            f"/api/scores/snapshot/{fixture_id}",
            headers=self._get_auth_headers()
        )
        response.raise_for_status()
        data = response.json()
        return [ScoreEntry.from_dict(s) for s in data]
    
    async def stream_odds_updates(self, fixture_ids: list[int]) -> AsyncGenerator[OddsEntry, None]:
        """
        Stream real-time odds updates for fixtures.
        
        Args:
            fixture_ids: List of fixture IDs to stream
            
        Yields:
            OddsEntry objects as they update
            
        Note: Requires paid tier for real-time streaming.
        """
        # Stream endpoint - requires paid tier
        response = await self.http.stream(
            "GET",
            "/api/odds/stream",
            headers={**self._get_auth_headers(), "Accept": "text/event-stream"},
            params={"fixtureIds": ",".join(map(str, fixture_ids))}
        )
        async for line in response.aiter_lines():
            if line.startswith("data:"):
                data = json.loads(line[5:])
                yield OddsEntry.from_dict(data)


# =============================================================================
# LMSR Q-Value Calculator
# =============================================================================

# SCALE for Q32.32 fixed-point arithmetic (2^32)
SCALE = 1 << 32  # 4294967296

# Default B parameter (100 USDC in raw lamports)
DEFAULT_B = 100_000_000

# Bounds for valid prices (1% to 99%)
MIN_PRICE_BPS = 100  # 1%
MAX_PRICE_BPS = 9900  # 99%


def price_to_fp(price: float) -> int:
    """
    Convert decimal price (0-1) to Q32.32 fixed point.
    
    Args:
        price: Probability between 0 and 1
        
    Returns:
        Fixed-point representation
    """
    if price <= 0 or price >= 1:
        raise ValueError("Price must be between 0 and 1 (exclusive)")
    return int(price * SCALE)


def fp_to_price(fp: int) -> float:
    """Convert Q32.32 fixed point to decimal price."""
    return fp / SCALE


def lmsr_price_fp(q_values: list[int], b: int, outcome_idx: int) -> int:
    """
    Compute LMSR price for an outcome in Q32.32.
    
    Formula: price_i = exp(q_i/B) / sum(exp(q_j/B))
    
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
    
    # Compute exp((q_i - max_q) / B) for each outcome
    exps = []
    for q in q_values:
        if q == max_q:
            exps.append(SCALE)  # exp(0) = 1
        else:
            diff = max_q - q
            # exponent = -(diff / B) in Q32.32
            # exp(-x) approximation for Q32.32
            exponent = (diff * SCALE) // b
            exps.append(_exp_neg_fp(exponent))
    
    sum_exp = sum(exps)
    if sum_exp == 0:
        return SCALE // n  # Equal price
    
    return (exps[outcome_idx] * SCALE) // sum_exp


def _exp_neg_fp(x: int) -> int:
    """
    Approximation for exp(-x) where x is in Q32.32.
    Uses Taylor series: exp(-x) ≈ 1 - x + x²/2 - x³/6 + x⁴/24
    """
    if x == 0:
        return SCALE
    
    # For large x, exp is near 0
    max_exp = SCALE * 10
    if x > max_exp:
        return 0
    
    # Taylor series approximation
    x_abs = x
    x_sq = (x_abs * x_abs) // SCALE
    x_cu = (x_sq * x_abs) // SCALE
    x_4 = (x_cu * x_abs) // SCALE
    
    result = SCALE      # 1
    result -= x_abs     # -x
    result += x_sq // 2  # +x²/2
    result -= x_cu // 6  # -x³/6
    result += x_4 // 24  # +x⁴/24
    
    return max(0, result)


def compute_q_values_2_outcomes(price0: float, b: int = DEFAULT_B) -> list[int]:
    """
    Compute q_values for 2-outcome market using closed-form solution.
    
    Formula: q_0 = B * ln(price_0 / (1 - price_0)), q_1 = 0
    
    Args:
        price0: Probability of outcome 0 (0-1)
        b: LMSR B parameter
        
    Returns:
        [q_0, q_1] in Q32.32
    """
    if price0 <= 0 or price0 >= 1:
        raise ValueError("Price must be between 0 and 1")
    
    ratio = price0 / (1 - price0)
    ln_ratio = math.log(ratio)
    ln_ratio_fp = int(ln_ratio * SCALE)
    q0 = (b * ln_ratio_fp) // SCALE
    
    return [q0, 0]


def compute_q_values_from_prices(
    target_prices: list[float],
    b: int = DEFAULT_B,
    max_iterations: int = 100,
    tolerance: int = 1
) -> list[int]:
    """
    Compute q_values from target probabilities using gradient descent.
    
    Args:
        target_prices: Target probabilities (must sum to ~1)
        b: LMSR B parameter
        max_iterations: Max optimization iterations
        tolerance: Convergence tolerance in Q32.32 units
        
    Returns:
        q_values in Q32.32
    """
    n = len(target_prices)
    if n < 2:
        raise ValueError("Need at least 2 outcomes")
    
    targets = [price_to_fp(p) for p in target_prices]
    
    # Verify prices sum to ~1
    sum_target = sum(targets)
    tolerance_sum = SCALE // 100  # 1% tolerance
    if abs(sum_target - SCALE) > tolerance_sum:
        raise ValueError(f"Target prices must sum to ~1, got {sum_target / SCALE:.4f}")
    
    # Initialize q_values to zero (equal distribution)
    q_values = [0] * n
    
    # Learning rate
    lr = SCALE // 1000  # 0.001
    prev_error = float('inf')
    
    for _ in range(max_iterations):
        # Compute current prices
        prices = [lmsr_price_fp(q_values, b, i) for i in range(n)]
        
        # Compute total error
        total_error = sum(abs(prices[i] - targets[i]) for i in range(n))
        
        # Check convergence
        if total_error < tolerance:
            break
        
        # Update q_values using gradient descent
        for i in range(n):
            error = prices[i] - targets[i]
            delta = (lr * error) // SCALE
            q_values[i] -= delta
            
            # Clamp to reasonable range
            max_q = b * 100
            min_q = -b * 100
            q_values[i] = max(min_q, min(max_q, q_values[i]))
        
        # Divergence check
        if total_error > prev_error * 3 and lr > SCALE // 10000:
            break
        prev_error = total_error
    
    return q_values


def compute_q_values_from_odds(
    odds_entries: list[OddsEntry],
    market_type: str,
    b: int = DEFAULT_B
) -> tuple[list[int], list[tuple[str, float]]]:
    """
    Compute q_values from TxODDS odds entries.
    
    Args:
        odds_entries: List of OddsEntry from TxODDS
        market_type: Market type to filter (e.g., "1X2")
        b: LMSR B parameter
        
    Returns:
        Tuple of (q_values, odds_info) where odds_info is list of (selection, probability)
    """
    # Filter to specified market type
    market_odds = [o for o in odds_entries if o.market_type == market_type]
    
    if not market_odds:
        raise ValueError(f"No odds found for market type: {market_type}")
    
    # Sort by selection for consistent ordering
    market_odds.sort(key=lambda x: x.selection)
    
    # Extract de-marginated probabilities
    # odds_demarginated is the true probability (0-1), not decimal odds
    probabilities = []
    for o in market_odds:
        if o.odds_demarginated > 0:
            # If it's already a probability (0-1), use it directly
            prob = o.odds_demarginated if o.odds_demarginated <= 1 else 1 / o.odds_demarginated
        else:
            # Otherwise convert from decimal odds
            prob = 1 / o.odds_marginated
        probabilities.append(prob)
    
    # Validate
    total = sum(probabilities)
    if total <= 0 or total > 2:
        raise ValueError(f"Invalid probabilities from odds: sum = {total}")
    
    # Normalize to sum to 1
    normalized = [p / total for p in probabilities]
    
    # Compute q_values
    if len(normalized) == 2:
        q_values = compute_q_values_2_outcomes(normalized[0], b)
    else:
        q_values = compute_q_values_from_prices(normalized, b)
    
    odds_info = [(o.selection, p) for o, p in zip(market_odds, normalized)]
    
    return q_values, odds_info


def validate_q_values(
    q_values: list[int],
    b: int = DEFAULT_B
) -> tuple[bool, list[float], list[str]]:
    """
    Validate that q_values produce reasonable prices.
    
    Args:
        q_values: q_values to validate
        b: LMSR B parameter
        
    Returns:
        Tuple of (valid, prices, errors)
    """
    errors = []
    prices = []
    
    n = len(q_values)
    min_price = SCALE // 100  # 1%
    max_price = (SCALE * 99) // 100  # 99%
    
    for i in range(n):
        try:
            price_fp = lmsr_price_fp(q_values, b, i)
            price = fp_to_price(price_fp)
            prices.append(price)
            
            if price < 0.01:
                errors.append(f"Outcome {i}: price {price:.4f} < 1%")
            if price > 0.99:
                errors.append(f"Outcome {i}: price {price:.4f} > 99%")
        except Exception as e:
            errors.append(f"Outcome {i}: failed to compute price - {e}")
    
    # Check sum
    total = sum(prices)
    if abs(total - 1) > 0.01:
        errors.append(f"Prices sum to {total:.4f}, expected ~1.0")
    
    return len(errors) == 0, prices, errors


def q_values_to_u64(q_values: list[int]) -> list[int]:
    """
    Convert q_values to Solana-compatible u64 format.
    
    Since q_values can be negative in LMSR but Solana uses unsigned,
    we shift so the minimum is 0 while preserving relative differences.
    """
    min_q = min(q_values)
    if min_q >= 0:
        return q_values
    return [q + abs(min_q) for q in q_values]


# =============================================================================
# Market Seeder Example
# =============================================================================

async def create_seeded_market(
    client: TxODDSClient,
    fixture: Fixture,
    market_type: str = "1X2",
    b: int = DEFAULT_B,
    lmsr_b_override: int = DEFAULT_B
) -> dict[str, Any]:
    """
    Create market parameters from a TxODDS fixture.
    
    Args:
        client: TxODDS client
        fixture: Fixture to create market for
        market_type: Market type (e.g., "1X2")
        b: B parameter for q_value computation
        lmsr_b_override: B parameter for on-chain LMSR
        
    Returns:
        Dict with market parameters ready for create_market instruction
    """
    # Fetch odds
    odds = await client.get_odds_snapshot(fixture.id)
    
    # Compute q_values
    q_values, odds_info = compute_q_values_from_odds(odds, market_type, b)
    
    # Validate
    valid, prices, errors = validate_q_values(q_values, b)
    if not valid:
        raise ValueError(f"Invalid q_values: {errors}")
    
    # Convert to on-chain format
    on_chain_q_values = q_values_to_u64(q_values)
    
    return {
        "title": f"{fixture.participant1} vs {fixture.participant2}",
        "description": f"{market_type} market for {fixture.league}",
        "start_time": fixture.start_time,
        "num_outcomes": len(q_values),
        "lmsr_b": lmsr_b_override,
        "initial_q_values": on_chain_q_values,
        "prices": prices,
        "odds_info": odds_info,
    }
