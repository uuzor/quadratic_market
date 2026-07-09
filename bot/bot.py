"""
Quadratic Market sports bot for football markets.

Lifecycle per fixture:
  1. [create_markets]   Fetch upcoming fixtures → create market groups and markets
                        with odds-seeded q_values
  2. [suspend_markets]  At start_time → suspend_market (no more bets)
  3. [settle_markets]   After start_time + RESULT_DELAY_SECONDS → fetch score
                        → admin_override (oracle/admin signs)
  4. [finalize_markets] After challenge window → finalize_result
  5. [void_expired]     If oracle never settled → void_if_expired

Market types per fixture:
  - Match Result (category=0): Home/Draw/Away (3 outcomes)
  - Both Teams To Score (category=1): Yes/No (2 outcomes)
  - Over/Under 2.5 (category=2): Over/Under (2 outcomes)

Run continuously:
    python bot.py

Or run a single pass (useful for cron):
    python bot.py --once
"""

from __future__ import annotations

import asyncio
import sys
import time
import argparse
import json
from pathlib import Path

import structlog
from apscheduler.schedulers.asyncio import AsyncIOScheduler

import config
from chain import ChainClient, load_keypair, market_pda
from odds_converter import decimal_odds_to_q_values, MarketType, FOOTBALL_MARKET_TYPES
from sports_api import OddsApiClient, Fixture, MarketOdds
from state import BotState, TrackedMarket, MarketStage

# ─── Logging ──────────────────────────────────────────────────────────────────

structlog.configure(
    wrapper_class=structlog.make_filtering_bound_logger(
        getattr(__import__("logging"), config.LOG_LEVEL, 20)
    ),
)
log = structlog.get_logger(__name__)

# Path to the compiled IDL (built by `anchor build`)
IDL_PATH = Path(__file__).parent / "idl.json"


# ─── Market type mapping ──────────────────────────────────────────────────────

# Map API market key to category number
MARKET_KEY_TO_CATEGORY = {
    "h2h": 0,     # Match Result (Home/Draw/Away)
    "btts": 1,    # Both Teams To Score
    "totals": 2,  # Over/Under goals
}

MARKET_CATEGORY_TO_OUTCOMES = {
    0: 3,  # Match Result: Home, Draw, Away
    1: 2,  # BTTS: Yes, No
    2: 2,  # Totals: Over, Under
}


# ─── Bot tasks ────────────────────────────────────────────────────────────────

async def task_sync_onchain_markets(chain: ChainClient, state: BotState) -> None:
    """
    Sync market state from on-chain data.
    This catches any markets that were created or updated directly on-chain.
    """
    try:
        cfg = await chain.fetch_global_config()
        next_market_id = int(cfg.next_market_id)
        
        # Scan through all market IDs (up to 100 for safety)
        for market_id in range(max(1, next_market_id - 100), next_market_id):
            if state.is_market_id_tracked(market_id):
                continue
            
            try:
                market_data = await chain.fetch_market(market_id)
                status = chain.get_market_status(market_data)
                market_info = chain.get_market_info(market_data)
                
                # Try to find matching fixture in state or create placeholder
                event_id = f"chain_{market_id}"
                
                # Add to tracking if market is active
                if status in ("open", "suspended"):
                    state.add_from_chain(
                        market_id=market_id,
                        event_id=event_id,
                        market_data={
                            "sport_key": "unknown",
                            "category": 0,
                            "num_outcomes": market_info["num_outcomes"],
                            "start_time": market_info["start_time"],
                            "home_team": market_info["title"].split(" vs ")[0] if " vs " in market_info["title"] else "Unknown",
                            "away_team": market_info["title"].split(" vs ")[1] if " vs " in market_info["title"] else "Unknown",
                            "challenge_window": 60,
                            "settlement_deadline": 7200,
                        }
                    )
                    log.info("synced_onchain_market", market_id=market_id, status=status)
                    
            except Exception as e:
                # Market doesn't exist or error reading
                pass
                
    except Exception as exc:
        log.error("sync_onchain_markets_failed", error=str(exc))


async def task_create_markets(chain: ChainClient, api: OddsApiClient, state: BotState) -> None:
    """
    Fetch upcoming fixtures and create market groups and markets.
    Each market is seeded with q_values derived from API odds.
    """
    fixtures = await api.upcoming_fixtures(config.MARKET_LOOKAHEAD_SECONDS)

    for fix in fixtures:
        if state.is_tracked(fix.event_id):
            continue

        # Create a market group for this fixture
        group_id = hash(fix.event_id) % (2**63)  # Generate deterministic group ID
        try:
            await chain.create_market_group(group_id, f"{fix.home_team} vs {fix.away_team}")
            log.info("market_group_created", event_id=fix.event_id, group_id=group_id)
        except Exception as exc:
            # Group might already exist
            log.debug("market_group_exists", event_id=fix.event_id, error=str(exc))

        # Create multiple market types per fixture
        markets_created = []
        for market_type in FOOTBALL_MARKET_TYPES:
            api_key = market_type.key
            if api_key not in fix.markets:
                log.debug("skipping_market_type_no_odds", fixture=fix.event_id, market_key=api_key)
                continue

            market_odds = fix.markets[api_key]
            num_outcomes = MARKET_CATEGORY_TO_OUTCOMES.get(market_type.category, 2)
            home = fix.home_team
            away = fix.away_team

            # Build title and description
            title_suffix = market_type.name
            if len(title_suffix) > 100:
                title_suffix = title_suffix[:97] + "..."
            title = f"{home} vs {away} - {title_suffix}"
            desc = f"{fix.sport_key} | {home} vs {away} | {market_type.name}"

            # Convert odds to q_values
            if market_type.category == 0:
                # Match Result: 3-way (Home/Draw/Away)
                odds_list = [market_odds.home_odds, market_odds.draw_odds, market_odds.away_odds]
            else:
                # 2-way markets
                odds_list = [market_odds.home_odds, market_odds.away_odds]

            # Filter out None or zero odds
            valid_odds = [o for o in odds_list if o and o > 0]
            if len(valid_odds) < num_outcomes:
                log.warning("insufficient_odds", fixture=fix.event_id, market_key=api_key, valid_count=len(valid_odds))
                continue

            q_values = decimal_odds_to_q_values(valid_odds)

            try:
                market_id, tx_sig = await chain.create_market(
                    start_time=fix.start_time,
                    num_outcomes=num_outcomes,
                    title=title[:128],
                    description=desc[:256],
                    category=market_type.category,
                    initial_q_values=q_values,
                )
                
                # Add market to group
                try:
                    await chain.add_market_to_group(
                        group_id=group_id,
                        market_id=market_id,
                        outcome_ids=list(range(num_outcomes)),
                    )
                except Exception as e:
                    log.warning("add_market_to_group_failed", market_id=market_id, error=str(e))
                
                markets_created.append({
                    "market_id": market_id,
                    "category": market_type.category,
                    "num_outcomes": num_outcomes,
                })
                log.info(
                    "market_created_with_odds",
                    event_id=fix.event_id,
                    market_id=market_id,
                    category=market_type.category,
                    num_outcomes=num_outcomes,
                    q_values=q_values[:num_outcomes],
                    tx=tx_sig[:20],
                )
            except Exception as exc:
                log.error("create_market_failed", event_id=fix.event_id, market_key=api_key, error=str(exc))
                continue

        # Add tracked market with all created market IDs
        if markets_created:
            state.add(TrackedMarket(
                event_id=fix.event_id,
                sport_key=fix.sport_key,
                market_id=markets_created[0]["market_id"],  # Primary market ID
                primary_category=markets_created[0]["category"],
                num_outcomes=markets_created[0]["num_outcomes"],
                start_time=fix.start_time,
                stage=MarketStage.MINTS_INIT,  # Assume mints are init
                home_team=home,
                away_team=away,
                sub_markets=markets_created,  # All markets for this fixture
                league=fix.sport_key,
            ))


async def task_suspend_markets(chain: ChainClient, state: BotState) -> None:
    """
    Suspend markets whose start_time has passed — closes betting.
    Uses the smarter timing from state.py.
    """
    now = int(time.time())
    markets_to_suspend = state.get_markets_needing_suspend(now)
    
    if not markets_to_suspend:
        return
        
    log.info("suspending_markets", count=len(markets_to_suspend))
    
    for m in markets_to_suspend:
        try:
            # Suspend primary market
            await chain.suspend_market(m.market_id)
            
            # Suspend all sub-markets
            if m.sub_markets:
                for sub in m.sub_markets:
                    if sub["market_id"] != m.market_id:
                        try:
                            await chain.suspend_market(sub["market_id"])
                        except Exception as e:
                            log.warning("suspend_sub_market_failed", sub_market_id=sub["market_id"], error=str(e))
            
            state.advance(m.event_id, MarketStage.SUSPENDED)
            log.info("market_suspended", event_id=m.event_id, market_id=m.market_id)
        except Exception as exc:
            log.error("suspend_failed", market_id=m.market_id, error=str(exc))


async def task_settle_markets(chain: ChainClient, api: OddsApiClient, state: BotState) -> None:
    """
    For suspended markets past the result delay, fetch the score and settle.
    Uses admin_override for direct settlement (simpler than propose+finalize).
    """
    now = int(time.time())
    markets_ready = state.get_markets_ready_to_propose(now)
    
    if not markets_ready:
        return

    log.info("settling_markets", count=len(markets_ready))

    # Group by sport to minimise API calls
    by_sport: dict[str, list[TrackedMarket]] = {}
    for m in markets_ready:
        by_sport.setdefault(m.sport_key, []).append(m)

    for sport, markets in by_sport.items():
        results = await api.completed_scores(sport, days_from=7)
        result_map = {r.event_id: r for r in results}

        for m in markets:
            result = result_map.get(m.event_id)
            if result is None:
                log.info("score_not_yet_available", market_id=m.market_id, event_id=m.event_id)
                continue
            if not result.completed:
                continue

            try:
                # Use admin_override for direct settlement
                await chain.admin_override(m.market_id, result.winning_outcome)
                
                # Settle all sub-markets
                if m.sub_markets:
                    for sub in m.sub_markets:
                        if sub["market_id"] != m.market_id:
                            try:
                                # Determine winning outcome for this market type
                                sub_winning = map_winning_outcome(
                                    result.winning_outcome,
                                    m.primary_category,
                                    sub["category"],
                                    m.home_team,
                                    m.away_team,
                                )
                                if sub_winning is not None:
                                    await chain.admin_override(sub["market_id"], sub_winning)
                            except Exception as e:
                                log.warning("settle_sub_market_failed", sub_market_id=sub["market_id"], error=str(e))
                
                state.advance(
                    m.event_id,
                    MarketStage.PROPOSED,
                    proposed_outcome=result.winning_outcome,
                )
                log.info("market_settled", event_id=m.event_id, outcome=result.winning_outcome)
            except Exception as exc:
                log.error("settle_market_failed", market_id=m.market_id, error=str(exc))


def map_winning_outcome(
    main_winning_outcome: int,
    main_category: int,
    sub_category: int,
    home_team: str,
    away_team: str,
) -> int | None:
    """
    Map the main match result to the winning outcome for a sub-market.
    
    For Match Result (category=0):
      outcome 0 = Home wins
      outcome 1 = Draw  
      outcome 2 = Away wins
    
    For BTTS (category=1):
      Need actual scores to determine if both teams scored
      For now, return None (skip settlement)
    
    For Totals (category=2):
      Need actual scores to determine total goals
      For now, return None (skip settlement)
    """
    if main_category == 0:
        return main_winning_outcome
    elif main_category == sub_category:
        return main_winning_outcome
    
    # Can't determine for BTTS/Totals without scores
    return None


async def task_finalize_markets(chain: ChainClient, state: BotState) -> None:
    """
    Finalize markets that are in PROPOSED stage and past the challenge window.
    """
    now = int(time.time())
    markets_to_finalize = state.get_markets_ready_to_finalize(now)
    
    if not markets_to_finalize:
        return
        
    log.info("finalizing_markets", count=len(markets_to_finalize))

    for m in markets_to_finalize:
        try:
            await chain.finalize_result(m.market_id)
            
            # Finalize all sub-markets
            if m.sub_markets:
                for sub in m.sub_markets:
                    if sub["market_id"] != m.market_id:
                        try:
                            await chain.finalize_result(sub["market_id"])
                        except Exception as e:
                            log.warning("finalize_sub_market_failed", sub_market_id=sub["market_id"], error=str(e))
            
            state.advance(m.event_id, MarketStage.FINALIZED)
            log.info("market_finalized", event_id=m.event_id, market_id=m.market_id)
        except Exception as exc:
            log.error("finalize_failed", market_id=m.market_id, error=str(exc))


async def task_void_expired(chain: ChainClient, state: BotState) -> None:
    """
    Void markets where the oracle never proposed a result within the deadline.
    """
    now = int(time.time())
    expired_markets = state.get_markets_expired(now)
    
    if not expired_markets:
        return
        
    log.info("voiding_expired_markets", count=len(expired_markets))

    for m in expired_markets:
        try:
            await chain.void_if_expired(m.market_id)
            
            # Void all sub-markets
            if m.sub_markets:
                for sub in m.sub_markets:
                    if sub["market_id"] != m.market_id:
                        try:
                            await chain.void_if_expired(sub["market_id"])
                        except Exception as e:
                            log.warning("void_sub_market_failed", sub_market_id=sub["market_id"], error=str(e))
            
            state.advance(m.event_id, MarketStage.VOIDED)
            log.info("market_voided", event_id=m.event_id, market_id=m.market_id)
        except Exception as exc:
            log.error("void_expired_failed", market_id=m.market_id, error=str(exc))


async def task_cleanup_old(state: BotState) -> None:
    """
    Clean up old finalized/voided markets from state.
    """
    removed = state.cleanup_old(max_age_days=7)
    if removed > 0:
        log.info("cleaned_up_old_markets", count=removed)


async def task_status_report(state: BotState) -> None:
    """
    Log current status of tracked markets.
    """
    stats = state.get_stats()
    log.info(
        "market_status_report",
        total=stats["total"],
        by_stage=stats["by_stage"],
        upcoming_24h=stats["upcoming_24h"],
        past_start=stats["past_start"],
    )


# ─── Main loop ────────────────────────────────────────────────────────────────

async def run_once(chain: ChainClient, api: OddsApiClient, state: BotState) -> None:
    """Execute one full pass of all bot tasks in dependency order."""
    log.info("bot_pass_start")
    
    # Sync state from on-chain
    if config.SYNC_ONCHAIN_MARKETS:
        await task_sync_onchain_markets(chain, state)
    
    # Create new markets
    await task_create_markets(chain, api, state)
    
    # Handle market lifecycle
    await task_suspend_markets(chain, state)
    await task_settle_markets(chain, api, state)
    await task_finalize_markets(chain, state)
    await task_void_expired(chain, state)
    
    # Cleanup and reporting
    await task_cleanup_old(state)
    await task_status_report(state)
    
    log.info("bot_pass_complete")


async def main(once: bool = False) -> None:
    if not IDL_PATH.exists():
        log.error("idl_not_found", path=str(IDL_PATH))
        log.error("Run `anchor build` first to generate the IDL.")
        sys.exit(1)

    operator_kp = load_keypair(config.OPERATOR_KEYPAIR_PATH)
    oracle_kp = load_keypair(config.ORACLE_KEYPAIR_PATH)

    chain = await ChainClient.create(
        rpc_url=config.RPC_URL,
        idl_path=IDL_PATH,
        program_id_str=config.PROGRAM_ID,
        operator_kp=operator_kp,
        oracle_kp=oracle_kp,
        base_mint_str=config.BASE_MINT,
    )
    api = OddsApiClient(config.ODDS_API_KEY, config.SPORTS)
    state = BotState()

    try:
        # Print initial status
        stats = state.get_stats()
        log.info(
            "bot_initialized",
            program_id=config.PROGRAM_ID,
            rpc_url=config.RPC_URL,
            tracked_markets=stats["total"],
        )

        if once:
            await run_once(chain, api, state)
            return

        scheduler = AsyncIOScheduler()
        
        # Main task loop
        scheduler.add_job(
            run_once,
            "interval",
            seconds=config.POLL_INTERVAL_SECONDS,
            args=[chain, api, state],
            id="bot_pass",
            max_instances=1,
            coalesce=True,
        )
        
        # Status reporting every minute
        scheduler.add_job(
            task_status_report,
            "interval",
            seconds=60,
            args=[state],
            id="status_report",
        )
        
        scheduler.start()
        log.info("bot_started", interval_seconds=config.POLL_INTERVAL_SECONDS)

        # Run immediately on startup
        await run_once(chain, api, state)

        # Keep running until interrupted
        try:
            while True:
                await asyncio.sleep(60)
        except (KeyboardInterrupt, SystemExit):
            log.info("bot_stopping")
            scheduler.shutdown()
    finally:
        await chain.close()
        await api.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Quadratic Market sports bot")
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run a single pass and exit (useful for cron jobs)",
    )
    parser.add_argument(
        "--status",
        action="store_true",
        help="Show current market status and exit",
    )
    args = parser.parse_args()
    
    if args.status:
        state = BotState()
        stats = state.get_stats()
        print(json.dumps(stats, indent=2))
    else:
        asyncio.run(main(once=args.once))
