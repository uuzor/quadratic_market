"""
Persistent bot state stored as a JSON file.

Tracks the mapping between The-Odds-API event IDs and on-chain market IDs,
plus the lifecycle stage of each market so the bot knows what to do next.

Supports multiple market types per fixture (Match Result, BTTS, Over/Under).
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, asdict, field
from enum import Enum
from pathlib import Path
from typing import Optional

import structlog

import config

log = structlog.get_logger(__name__)

STATE_FILE = Path(__file__).parent / "bot_state.json"


class MarketStage(str, Enum):
    CREATED = "created"         # market created on-chain, mints not yet initialised
    MINTS_INIT = "mints_init"   # outcome mints initialised, open for trading
    SUSPENDED = "suspended"     # match started, betting closed
    PROPOSED = "proposed"       # oracle proposed result, in challenge window
    FINALIZED = "finalized"     # result finalized, market settled
    VOIDED = "voided"           # market voided (oracle silent / admin action)


@dataclass
class TrackedMarket:
    """
    Tracks a single fixture's markets on-chain.
    
    Each fixture can have multiple market types (Match Result, BTTS, Over/Under).
    The primary_market_id points to the main 3-way market.
    sub_markets contains all markets for this fixture.
    """
    event_id: str           # The-Odds-API event ID (external reference)
    sport_key: str
    market_id: int          # Primary on-chain market ID (usually Match Result)
    primary_category: int   # Category of primary market (0=3-way, 1=BTTS, 2=Totals)
    num_outcomes: int       # Number of outcomes for primary market
    start_time: int         # Unix timestamp when match starts
    stage: MarketStage
    home_team: str
    away_team: str
    proposed_outcome: Optional[int] = None
    created_at: int = field(default_factory=lambda: int(time.time()))
    # Sub-markets: list of {market_id, category, num_outcomes}
    sub_markets: list = field(default_factory=list)
    
    # Settlement tracking
    suspended_at: Optional[int] = None          # When market was suspended
    proposed_at: Optional[int] = None          # When result was proposed
    finalized_at: Optional[int] = None        # When result was finalized
    voided_at: Optional[int] = None           # When market was voided
    
    # Timing metadata
    challenge_window: int = 60                # Challenge window in seconds
    settlement_deadline: int = 7200           # Max time before void in seconds
    
    # Odds metadata
    source: str = "the-odds-api"             # Odds source (the-odds-api, txodds)
    q_values: Optional[list] = None          # Initial q_values used for seeding
    
    # Additional metadata
    league: Optional[str] = None             # League/competition name
    odds_info: Optional[dict] = None         # Odds details for debugging


class BotState:
    """
    Thread-safe (single-process) state manager.
    Persists to disk after every mutation.
    """

    def __init__(self, path: Path = None) -> None:
        self._path = path or config.STATE_FILE
        # event_id -> TrackedMarket
        self._markets: dict[str, TrackedMarket] = {}
        # market_id (int) -> event_id for quick lookup
        self._market_id_index: dict[int, str] = {}
        self._load()

    # ── Persistence ───────────────────────────────────────────────────────────

    def _load(self) -> None:
        if not self._path.exists():
            log.info("state_file_not_found", path=str(self._path))
            return
        try:
            raw = json.loads(self._path.read_text())
            for item in raw.get("markets", []):
                # Handle sub_markets as list of dicts
                sub_markets = item.get("sub_markets", [])
                m = TrackedMarket(
                    event_id=item["event_id"],
                    sport_key=item["sport_key"],
                    market_id=item["market_id"],
                    primary_category=item.get("primary_category", 0),
                    num_outcomes=item["num_outcomes"],
                    start_time=item["start_time"],
                    stage=MarketStage(item["stage"]),
                    home_team=item["home_team"],
                    away_team=item["away_team"],
                    proposed_outcome=item.get("proposed_outcome"),
                    created_at=item.get("created_at", int(time.time())),
                    sub_markets=sub_markets,
                    suspended_at=item.get("suspended_at"),
                    proposed_at=item.get("proposed_at"),
                    finalized_at=item.get("finalized_at"),
                    voided_at=item.get("voided_at"),
                    challenge_window=item.get("challenge_window", 60),
                    settlement_deadline=item.get("settlement_deadline", 7200),
                    source=item.get("source", "the-odds-api"),
                    q_values=item.get("q_values"),
                    league=item.get("league"),
                    odds_info=item.get("odds_info"),
                )
                self._markets[m.event_id] = m
                self._market_id_index[m.market_id] = m.event_id
                # Index sub-markets too
                for sub in sub_markets:
                    self._market_id_index[sub["market_id"]] = m.event_id
            log.info("state_loaded", markets=len(self._markets), path=str(self._path))
        except Exception as exc:
            log.warning("state_load_failed", error=str(exc))

    def _save(self) -> None:
        data = {"markets": [asdict(m) for m in self._markets.values()]}
        self._path.write_text(json.dumps(data, indent=2))

    # ── Queries ───────────────────────────────────────────────────────────────

    def is_tracked(self, event_id: str) -> bool:
        return event_id in self._markets
    
    def is_market_id_tracked(self, market_id: int) -> bool:
        """Check if market_id is being tracked."""
        return market_id in self._market_id_index

    def get(self, event_id: str) -> Optional[TrackedMarket]:
        return self._markets.get(event_id)
    
    def get_by_market_id(self, market_id: int) -> Optional[TrackedMarket]:
        """Get TrackedMarket by on-chain market_id."""
        event_id = self._market_id_index.get(market_id)
        if event_id:
            return self._markets.get(event_id)
        return None

    def all_in_stage(self, stage: MarketStage) -> list[TrackedMarket]:
        return [m for m in self._markets.values() if m.stage == stage]
    
    def all_pending_settlement(self) -> list[TrackedMarket]:
        """Get all markets that should be checked for settlement."""
        return [m for m in self._markets.values() 
                if m.stage in (MarketStage.SUSPENDED, MarketStage.PROPOSED)]
    
    def all_active(self) -> list[TrackedMarket]:
        """Get all markets that are not finalized or voided."""
        return [m for m in self._markets.values() 
                if m.stage not in (MarketStage.FINALIZED, MarketStage.VOIDED)]

    def by_market_id(self, market_id: int) -> Optional[TrackedMarket]:
        for m in self._markets.values():
            if m.market_id == market_id:
                return m
            # Also check sub-markets
            if m.sub_markets:
                for sub in m.sub_markets:
                    if sub.get("market_id") == market_id:
                        return m
        return None
    
    def get_markets_needing_suspend(self, now: int) -> list[TrackedMarket]:
        """Get markets that need to be suspended (start_time + grace period passed)."""
        grace = config.SUSPEND_GRACE_PERIOD
        return [m for m in self.all_in_stage(MarketStage.MINTS_INIT) 
                if now >= m.start_time + grace]
    
    def get_markets_ready_to_propose(self, now: int) -> list[TrackedMarket]:
        """Get suspended markets that are ready for result proposal."""
        delay = config.RESULT_DELAY_SECONDS
        return [m for m in self.all_in_stage(MarketStage.SUSPENDED) 
                if now >= m.start_time + delay]
    
    def get_markets_ready_to_finalize(self, now: int) -> list[TrackedMarket]:
        """Get proposed markets that are past the challenge window."""
        return [m for m in self.all_in_stage(MarketStage.PROPOSED) 
                if m.proposed_at and now >= m.proposed_at + m.challenge_window]
    
    def get_markets_expired(self, now: int) -> list[TrackedMarket]:
        """Get suspended markets that have exceeded the void deadline."""
        return [m for m in self.all_in_stage(MarketStage.SUSPENDED) 
                if now >= m.start_time + m.settlement_deadline]
    
    def get_upcoming(self, now: int, lookahead: int) -> list[TrackedMarket]:
        """Get markets starting within the lookahead window."""
        cutoff = now + lookahead
        return [m for m in self.all_active() 
                if now <= m.start_time <= cutoff]
    
    def get_stats(self) -> dict:
        """Get statistics about tracked markets."""
        stats = {
            "total": len(self._markets),
            "by_stage": {},
            "upcoming_24h": 0,
            "past_start": 0,
        }
        now = int(time.time())
        for stage in MarketStage:
            stats["by_stage"][stage.value] = len(self.all_in_stage(stage))
        stats["upcoming_24h"] = len(self.get_upcoming(now, 86400))
        stats["past_start"] = len([m for m in self.all_active() if m.start_time < now])
        return stats

    # ── Mutations ─────────────────────────────────────────────────────────────

    def add(self, market: TrackedMarket) -> None:
        self._markets[market.event_id] = market
        self._market_id_index[market.market_id] = market.event_id
        for sub in market.sub_markets or []:
            self._market_id_index[sub["market_id"]] = market.event_id
        self._save()
        log.info(
            "state_add",
            event_id=market.event_id,
            market_id=market.market_id,
            sub_markets=len(market.sub_markets or []),
            start_time=market.start_time,
        )
    
    def add_from_chain(self, market_id: int, event_id: str, market_data: dict) -> TrackedMarket:
        """Add or update a market from on-chain data."""
        market = TrackedMarket(
            event_id=event_id,
            sport_key=market_data.get("sport_key", "unknown"),
            market_id=market_id,
            primary_category=market_data.get("category", 0),
            num_outcomes=market_data.get("num_outcomes", 2),
            start_time=market_data.get("start_time", 0),
            stage=MarketStage.MINTS_INIT,  # Assume mints are init if we have on-chain data
            home_team=market_data.get("home_team", ""),
            away_team=market_data.get("away_team", ""),
            challenge_window=market_data.get("challenge_window", 60),
            settlement_deadline=market_data.get("settlement_deadline", 7200),
        )
        self.add(market)
        return market

    def advance(self, event_id: str, stage: MarketStage, **kwargs) -> None:
        m = self._markets[event_id]
        m.stage = stage
        now = int(time.time())
        
        # Track timing
        if stage == MarketStage.SUSPENDED:
            m.suspended_at = now
        elif stage == MarketStage.PROPOSED:
            m.proposed_at = now
        elif stage == MarketStage.FINALIZED:
            m.finalized_at = now
        elif stage == MarketStage.VOIDED:
            m.voided_at = now
        
        for k, v in kwargs.items():
            setattr(m, k, v)
        self._save()
        log.info("state_advance", event_id=event_id, stage=stage.value, timing=now)
    
    def sync_from_chain(self, market_id: int, chain_status: dict) -> bool:
        """Sync market state from on-chain data. Returns True if changed."""
        market = self.get_by_market_id(market_id)
        if not market:
            return False
        
        old_stage = market.stage
        status = chain_status.get("status", {})
        
        # Map on-chain status to our stages
        if "open" in status:
            if old_stage in (MarketStage.CREATED,):
                self.advance(market.event_id, MarketStage.MINTS_INIT)
                return True
        elif "suspended" in status:
            if old_stage in (MarketStage.CREATED, MarketStage.MINTS_INIT):
                self.advance(market.event_id, MarketStage.SUSPENDED)
                return True
        elif "proposed" in status:
            if old_stage == MarketStage.SUSPENDED:
                self.advance(market.event_id, MarketStage.PROPOSED, 
                            proposed_outcome=chain_status.get("winning_outcome"))
                return True
        elif "settled" in status:
            if old_stage in (MarketStage.SUSPENDED, MarketStage.PROPOSED):
                self.advance(market.event_id, MarketStage.FINALIZED)
                return True
        elif "voided" in status:
            if old_stage != MarketStage.VOIDED:
                self.advance(market.event_id, MarketStage.VOIDED)
                return True
        
        return False
    
    def remove(self, event_id: str) -> None:
        """Remove a market from tracking."""
        market = self._markets.get(event_id)
        if market:
            del self._market_id_index[market.market_id]
            for sub in market.sub_markets or []:
                self._market_id_index.pop(sub["market_id"], None)
            del self._markets[event_id]
            self._save()
            log.info("state_remove", event_id=event_id)
    
    def cleanup_old(self, max_age_days: int = 7) -> int:
        """Remove finalized/voided markets older than max_age_days. Returns count removed."""
        cutoff = int(time.time()) - (max_age_days * 86400)
        to_remove = []
        for m in self._markets.values():
            if m.stage in (MarketStage.FINALIZED, MarketStage.VOIDED):
                # Use finalized_at or voided_at, or fallback to created_at
                ts = (m.finalized_at or m.voided_at or m.created_at)
                if ts and ts < cutoff:
                    to_remove.append(m.event_id)
        
        for event_id in to_remove:
            self.remove(event_id)
        
        if to_remove:
            log.info("state_cleanup", removed=len(to_remove))
        return len(to_remove)
