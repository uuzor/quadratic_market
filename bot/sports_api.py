"""
Sports data client using The-Odds-API (https://the-odds-api.com) and TXOdds.

Free tier: 500 requests/month. The bot uses two endpoints:
  - /sports/{sport}/odds  — upcoming fixtures with odds (to create markets)
  - /sports/{sport}/scores — completed scores (to settle markets)

Supports multiple market types per fixture:
  - h2h: 3-way (Home/Draw/Away) for soccer
  - btts: Both Teams To Score (Yes/No)
  - totals: Over/Under goals

Odds are converted to LMSR q_values for market seeding.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Optional

import httpx
import structlog

from txodds import TxODDSClient, Fixture as TxFixture, ScoreEntry as TxScore

log = structlog.get_logger(__name__)

BASE_URL = "https://api.the-odds-api.com/v4"


@dataclass
class MarketOdds:
    """Odds for a single market type within a fixture."""
    key: str          # Market key: "h2h", "btts", "totals"
    home_odds: float # Decimal odds for first outcome
    away_odds: float  # Decimal odds for second outcome (or draw for 3-way)
    draw_odds: Optional[float] = None  # Only for 3-way markets


@dataclass
class Fixture:
    """A single upcoming match with multiple market types."""
    event_id: str           # The-Odds-API event ID
    sport_key: str
    home_team: str
    away_team: str
    start_time: int         # Unix timestamp (UTC)
    has_draw: bool          # True for soccer/3-way markets
    markets: dict[str, MarketOdds] = field(default_factory=dict)  # market_key -> MarketOdds
    source: str = "the-odds-api"  # Source of odds data
    league: Optional[str] = None  # League name


@dataclass
class SettledResult:
    """A completed match with a known winner."""
    event_id: str
    sport_key: str
    home_team: str
    away_team: str
    winning_outcome: int    # 0=home, 1=draw (3-way only), 2=away (or 1 for 2-way)
    completed: bool
    home_score: int = 0
    away_score: int = 0


# Sports that have a draw outcome (3-way markets)
THREE_WAY_SPORTS = {
    "soccer_epl",
    "soccer_uefa_champs_league",
    "soccer_uefa_europa_league",
    "soccer_spain_la_liga",
    "soccer_germany_bundesliga",
    "soccer_italy_serie_a",
    "soccer_france_ligue_one",
}


class OddsApiClient:
    def __init__(self, api_key: str, sports: list[str], txodds_token: Optional[str] = None) -> None:
        self._key = api_key
        self._sports = sports
        self._http = httpx.AsyncClient(base_url=BASE_URL, timeout=15.0)
        
        # TXOdds client for more accurate odds (optional)
        self._txodds: Optional[TxODDSClient] = None
        if txodds_token:
            try:
                self._txodds = TxODDSClient()
            except Exception as e:
                log.warning("txodds_init_failed", error=str(e))

    async def close(self) -> None:
        await self._http.aclose()
        if self._txodds:
            await self._txodds.close()

    # ── TXOdds integration ─────────────────────────────────────────────────────

    async def _fetch_txodds_fixtures(self) -> dict[int, TxFixture]:
        """Fetch fixtures from TXOdds."""
        if not self._txodds:
            return {}
        
        try:
            # Start session
            if not self._txodds.jwt:
                await self._txodds.start_guest_session()
            
            fixtures = await self._txodds.get_fixtures()
            return {f.id: f for f in fixtures}
        except Exception as e:
            log.warning("txodds_fetch_failed", error=str(e))
            return {}

    async def _fetch_txodds_scores(self, fixture_id: int) -> Optional[TxScore]:
        """Fetch scores from TXOdds for a specific fixture."""
        if not self._txodds:
            return None
        
        try:
            scores = await self._txodds.get_scores_snapshot(fixture_id)
            if scores:
                # Get the most recent full-time score
                for s in reversed(scores):
                    if s.score_type == "FULL_TIME":
                        return s
                return scores[-1]
        except Exception as e:
            log.warning("txodds_scores_failed", fixture_id=fixture_id, error=str(e))
        return None

    # ── Upcoming fixtures with all market types ───────────────────────────────

    async def upcoming_fixtures(self, lookahead_seconds: int) -> list[Fixture]:
        """
        Return fixtures starting within the next `lookahead_seconds`.
        Fetches all supported market types (h2h, btts, totals) per fixture.
        Uses the /odds endpoint to get odds for multiple markets.
        """
        now = int(time.time())
        cutoff = now + lookahead_seconds
        fixtures: list[Fixture] = []

        # Try TXOdds first if available
        txodds_fixtures = {}
        if self._txodds:
            txodds_fixtures = await self._fetch_txodds_fixtures()

        for sport in self._sports:
            try:
                # Fetch all available markets
                resp = await self._http.get(
                    f"/sports/{sport}/odds",
                    params={
                        "apiKey": self._key,
                        "regions": "eu,uk,us",
                        "markets": "h2h,btts,totals",
                        "oddsFormat": "decimal",
                        "dateFormat": "unix",
                    },
                )
                resp.raise_for_status()
                events = resp.json()
                remaining = resp.headers.get("x-requests-remaining", "?")
                log.debug("odds_api_response", sport=sport, events=len(events), remaining=remaining)

                for ev in events:
                    start = int(ev["commence_time"])
                    if start <= now or start > cutoff:
                        continue

                    # Parse all market types from this event
                    markets = {}
                    bookmakers = ev.get("bookmakers", [])
                    if bookmakers:
                        # Use first bookmaker's odds (could aggregate multiple)
                        bm = bookmakers[0]
                        for market in bm.get("markets", []):
                            key = market.get("key", "")
                            if key in ("h2h", "btts", "totals"):
                                outcomes = market.get("outcomes", [])
                                if len(outcomes) >= 2:
                                    odds_list = sorted(outcomes, key=lambda x: x.get("name", ""))
                                    # Parse outcomes based on market type
                                    if key == "h2h" and len(outcomes) == 3:
                                        # 3-way: home, draw, away
                                        home_odds = next((o["price"] for o in outcomes if o["name"] in (ev["home_team"], "Home", "Team 1")), 0)
                                        draw_odds = next((o["price"] for o in outcomes if o["name"] == "Draw"), 0)
                                        away_odds = next((o["price"] for o in outcomes if o["name"] in (ev["away_team"], "Away", "Team 2")), 0)
                                        if home_odds > 0 and away_odds > 0:
                                            markets[key] = MarketOdds(
                                                key=key,
                                                home_odds=home_odds,
                                                draw_odds=draw_odds if draw_odds > 0 else None,
                                                away_odds=away_odds,
                                            )
                                    elif key in ("btts", "totals") and len(outcomes) == 2:
                                        # 2-way: yes/no or over/under
                                        home_odds = outcomes[0]["price"]
                                        away_odds = outcomes[1]["price"]
                                        markets[key] = MarketOdds(
                                            key=key,
                                            home_odds=home_odds,
                                            away_odds=away_odds,
                                        )

                    # Get league name if available
                    league = None
                    if bookmakers:
                        league = bookmakers[0].get("title")

                    fixtures.append(Fixture(
                        event_id=ev["id"],
                        sport_key=sport,
                        home_team=ev["home_team"],
                        away_team=ev["away_team"],
                        start_time=start,
                        has_draw=sport in THREE_WAY_SPORTS,
                        markets=markets,
                        source="txodds" if txodds_fixtures else "the-odds-api",
                        league=league,
                    ))
            except httpx.HTTPStatusError as exc:
                log.warning("odds_api_error", sport=sport, status=exc.response.status_code)
            except Exception as exc:
                log.warning("odds_api_exception", sport=sport, error=str(exc))

        log.info("upcoming_fixtures", count=len(fixtures))
        return fixtures

    # ── Scores / results ───────────────────────────────────────────────────────

    async def completed_scores(self, sport: str, days_from: int = 1) -> list[SettledResult]:
        """
        Return completed matches for a sport from the last `days_from` days.
        """
        results: list[SettledResult] = []
        try:
            resp = await self._http.get(
                f"/sports/{sport}/scores",
                params={
                    "apiKey": self._key,
                    "daysFrom": days_from,
                    "dateFormat": "unix",
                },
            )
            resp.raise_for_status()
            events = resp.json()

            for ev in events:
                if not ev.get("completed"):
                    continue
                scores = ev.get("scores") or []
                if len(scores) < 2:
                    continue

                # scores is a list of {"name": team, "score": "N"} dicts
                score_map = {s["name"]: int(s["score"]) for s in scores}
                home = ev["home_team"]
                away = ev["away_team"]
                home_score = score_map.get(home, 0)
                away_score = score_map.get(away, 0)
                has_draw = sport in THREE_WAY_SPORTS

                if home_score > away_score:
                    winning_outcome = 0  # home
                elif away_score > home_score:
                    winning_outcome = 2 if has_draw else 1  # away
                else:
                    winning_outcome = 1 if has_draw else None  # draw

                if winning_outcome is None:
                    # Draw in a 2-way market — shouldn't happen but skip safely
                    log.warning("unexpected_draw_in_2way", event_id=ev["id"])
                    continue

                results.append(SettledResult(
                    event_id=ev["id"],
                    sport_key=sport,
                    home_team=home,
                    away_team=away,
                    winning_outcome=winning_outcome,
                    completed=True,
                    home_score=home_score,
                    away_score=away_score,
                ))
        except httpx.HTTPStatusError as exc:
            log.warning("scores_api_error", sport=sport, status=exc.response.status_code)
        except Exception as exc:
            log.warning("scores_api_exception", sport=sport, error=str(exc))

        return results
