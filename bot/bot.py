"""
Txodds Market Bot for Quadratic Market Protocol.

This bot:
1. Creates market groups (1X2, O/U, GG/NG) for upcoming fixtures
2. Updates odds until match start
3. Suspends markets at match start
4. Settles markets with TxLINE proof
5. Executes slip legs (backend function)
6. Settles and resolves slips

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
from pathlib import Path
from typing import List, Optional

import structlog
from apscheduler.schedulers.asyncio import AsyncIOScheduler

import config
from chain import ChainClient, load_keypair, market_pda
from txodds_api import (
    TxoddsApiClient, 
    Network, 
    TxoddsFixture,
    TxoddsResult,
    MarketType,
    derive_market_odds,
    odds_to_basis_points,
)
from state import BotState, TrackedMarket, TrackedMarketGroup, TrackedSlip, MarketStage, MarketType as StateMarketType

# ─── Logging ──────────────────────────────────────────────────────────────────

structlog.configure(
    wrapper_class=structlog.make_filtering_bound_logger(
        getattr(__import__("logging"), config.LOG_LEVEL, 20)
    ),
)
log = structlog.get_logger(__name__)

# Path to the compiled IDL (built by `anchor build`)
IDL_PATH = Path(__file__).parent.parent / "target" / "idl" / "quadratic_market.json"


# ─── Market Type Mapping ───────────────────────────────────────────────────

MARKET_TYPE_CONFIG = {
    "1x2": {
        "category": 0,
        "num_outcomes": 3,
        "market_type": "oneXTwo",
    },
    "over_under": {
        "category": 1,
        "num_outcomes": 2,
        "market_type": "overUnder",
    },
    "gg_ng": {
        "category": 2,
        "num_outcomes": 2,
        "market_type": "goalNoGoal",
    },
}


# ─── Bot Tasks ────────────────────────────────────────────────────────────────

async def task_create_markets(
    chain: ChainClient, 
    api: TxoddsApiClient, 
    state: BotState
) -> None:
    """
    Fetch upcoming fixtures and create market groups with 3 markets each:
    1X2, O/U 2.5, GG/NG
    """
    fixtures = await api.get_upcoming_fixtures(config.MARKET_LOOKAHEAD_DAYS)
    
    for fix in fixtures:
        if state.is_group_tracked(fix.fixture_id):
            continue
        
        log.info("creating_market_group", fixture_id=fix.fixture_id)
        
        # Get odds for the fixture
        odds_snapshot = await api.get_best_odds(fix.fixture_id)
        
        try:
            # Create market group
            group_id = await chain.next_market_id()
            title = f"{fix.home_team} vs {fix.away_team}"
            
            await chain.create_market_group(
                group_id=group_id,
                title=title,
                event_start_time=fix.start_time,
            )
            
            market_ids = []
            
            # Create 1X2 market
            odds_1x2 = derive_market_odds(fix.fixture_id, odds_snapshot, MarketType.ONE_X_TWO) if odds_snapshot else [20000, 35000, 30000]
            market_1x2_id, _ = await chain.create_market(
                start_time=fix.start_time,
                num_outcomes=3,
                title=f"1X2: {title}",
                description=f"{fix.sport_key}",
                category=0,
                market_type="oneXTwo",
                initial_odds=odds_1x2,
            )
            market_ids.append(market_1x2_id)
            
            # Create O/U 2.5 market
            odds_ou = derive_market_odds(fix.fixture_id, odds_snapshot, MarketType.OVER_UNDER) if odds_snapshot else [18000, 19000]
            market_ou_id, _ = await chain.create_market(
                start_time=fix.start_time,
                num_outcomes=2,
                title=f"O/U 2.5: {title}",
                description=f"{fix.sport_key}",
                category=1,
                market_type="overUnder",
                initial_odds=odds_ou,
            )
            market_ids.append(market_ou_id)
            
            # Create GG/NG market
            odds_ggng = derive_market_odds(fix.fixture_id, odds_snapshot, MarketType.GG_NG) if odds_snapshot else [17000, 20000]
            market_ggng_id, _ = await chain.create_market(
                start_time=fix.start_time,
                num_outcomes=2,
                title=f"GG/NG: {title}",
                description=f"{fix.sport_key}",
                category=2,
                market_type="goalNoGoal",
                initial_odds=odds_ggng,
            )
            market_ids.append(market_ggng_id)
            
            # Add markets to group
            for i, mkt_id in enumerate(market_ids):
                await chain.add_market_to_group(group_id, mkt_id, i)
            
            # Track group
            group = TrackedMarketGroup(
                fixture_id=fix.fixture_id,
                group_id=group_id,
                home_team=fix.home_team,
                away_team=fix.away_team,
                sport_key=fix.sport_key,
                start_time=fix.start_time,
                market_ids=market_ids,
                stage=MarketStage.MINTS_INIT,
            )
            state.add_group(group)
            
            # Track individual markets
            for i, mkt_id in enumerate(market_ids):
                mkt_type = ["1x2", "over_under", "gg_ng"][i]
                mkt_config = MARKET_TYPE_CONFIG[mkt_type]
                market = TrackedMarket(
                    fixture_id=fix.fixture_id,
                    market_id=mkt_id,
                    market_type=StateMarketType(mkt_type),
                    category=mkt_config["category"],
                    num_outcomes=mkt_config["num_outcomes"],
                    start_time=fix.start_time,
                    stage=MarketStage.MINTS_INIT,
                    title=f"{mkt_type}: {title}",
                    market_index=i,
                )
                state.add_market(market)
            
            log.info("market_group_created", 
                     fixture_id=fix.fixture_id, 
                     group_id=group_id,
                     markets=market_ids)
            
        except Exception as exc:
            log.error("create_market_group_failed", 
                      fixture_id=fix.fixture_id, 
                      error=str(exc))


async def task_update_odds(
    chain: ChainClient, 
    api: TxoddsApiClient, 
    state: BotState
) -> None:
    """
    Update odds for open markets based on latest txodds data.
    Only updates markets that haven't started yet.
    """
    now = int(time.time())
    
    # Get markets that are open for trading
    for market in state.all_markets_in_stage(MarketStage.MINTS_INIT):
        if now >= market.start_time:
            continue  # Market already started
        
        try:
            # Get latest odds from txodds
            odds_snapshot = await api.get_best_odds(market.fixture_id)
            if not odds_snapshot:
                continue
            
            # Derive odds for this market type
            mkt_type = MarketType(market.market_type.value.replace("-", "_"))
            new_odds = derive_market_odds(market.fixture_id, odds_snapshot, mkt_type)
            
            # Update odds
            await chain.update_market_odds(market.market_id, new_odds)
            log.info("odds_updated", 
                     market_id=market.market_id, 
                     odds=new_odds)
            
        except Exception as exc:
            log.error("update_odds_failed", 
                      market_id=market.market_id, 
                      error=str(exc))


async def task_suspend_markets(
    chain: ChainClient, 
    state: BotState
) -> None:
    """
    Suspend markets whose start_time has passed — closes betting.
    """
    now = int(time.time())
    
    for market in state.all_markets_in_stage(MarketStage.MINTS_INIT):
        if now < market.start_time:
            continue
        
        try:
            await chain.suspend_market(market.market_id)
            state.advance_market(market.market_id, MarketStage.SUSPENDED)
            log.info("market_suspended", market_id=market.market_id)
        except Exception as exc:
            log.error("suspend_market_failed", 
                      market_id=market.market_id, 
                      error=str(exc))


async def task_settle_markets(
    chain: ChainClient, 
    api: TxoddsApiClient, 
    state: BotState
) -> None:
    """
    Settlement is now proof-gated on-chain. The bot no longer submits the
    removed legacy settlement flow here.
    """
    for market in state.all_markets_in_stage(MarketStage.SUSPENDED):
        log.info("market_requires_proof_settlement",
                 market_id=market.market_id,
                 fixture_id=market.fixture_id)


async def task_void_expired(
    chain: ChainClient, 
    state: BotState
) -> None:
    """
    Void markets where no result was provided within the deadline.
    """
    cfg = await chain.fetch_global_config()
    deadline_seconds = int(cfg.settlement_deadline_seconds)
    now = int(time.time())
    
    for market in state.all_markets_in_stage(MarketStage.SUSPENDED):
        if now < market.start_time + deadline_seconds:
            continue
        
        try:
            await chain.void_if_expired(market.market_id)
            state.advance_market(market.market_id, MarketStage.VOIDED)
            log.info("market_voided", market_id=market.market_id)
        except Exception as exc:
            log.error("void_market_failed", 
                      market_id=market.market_id, 
                      error=str(exc))


async def task_settle_slips(
    chain: ChainClient, 
    state: BotState
) -> None:
    """
    Settle slip legs for markets that have been settled.
    This is called by the bot after markets are finalized.
    """
    for market in state.all_markets_in_stage(MarketStage.FINALIZED):
        # Find slips that include this market
        # Note: In practice, this would need to track which slips include which markets
        log.info("market_ready_for_slip_settlement", market_id=market.market_id)


# ─── Main Loop ────────────────────────────────────────────────────────────────

async def run_once(chain: ChainClient, api: TxoddsApiClient, state: BotState) -> None:
    """Execute one full pass of all bot tasks in dependency order."""
    log.info("bot_pass_start")
    
    # 1. Create new markets from upcoming fixtures
    await task_create_markets(chain, api, state)
    
    # 2. Update odds for open markets
    await task_update_odds(chain, api, state)
    
    # 3. Suspend markets at start time
    await task_suspend_markets(chain, state)
    
    # 4. Settle markets after delay
    await task_settle_markets(chain, api, state)

    # 5. Void expired markets
    await task_void_expired(chain, state)

    # 6. Settle slip legs
    await task_settle_slips(chain, state)

    log.info("bot_pass_complete")


async def main(once: bool = False) -> None:
    if not IDL_PATH.exists():
        log.error("idl_not_found", path=str(IDL_PATH))
        log.error("Run `anchor build` first to generate the IDL.")
        sys.exit(1)
    
    # Load keypairs
    operator_kp = load_keypair(config.OPERATOR_KEYPAIR_PATH)
    oracle_kp = load_keypair(config.ORACLE_KEYPAIR_PATH)
    
    # Initialize chain client
    chain = await ChainClient.create(
        rpc_url=config.RPC_URL,
        idl_path=IDL_PATH,
        program_id_str=config.PROGRAM_ID,
        operator_kp=operator_kp,
        oracle_kp=oracle_kp,
        base_mint_str=config.BASE_MINT,
    )
    
    # Initialize txodds API client
    network = Network.DEVNET if config.TXODDS_NETWORK == "devnet" else Network.MAINNET
    api = TxoddsApiClient(config.TXODDS_API_KEY, network)
    await api.authenticate()
    
    # Load state
    state = BotState()
    
    try:
        if once:
            await run_once(chain, api, state)
            return
        
        # Run on schedule
        scheduler = AsyncIOScheduler()
        scheduler.add_job(
            run_once,
            "interval",
            seconds=config.POLL_INTERVAL_SECONDS,
            args=[chain, api, state],
            id="bot_pass",
            max_instances=1,
            coalesce=True,
        )
        scheduler.start()
        log.info("bot_started", 
                 interval_seconds=config.POLL_INTERVAL_SECONDS,
                 sports=config.SPORTS)
        
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
    parser = argparse.ArgumentParser(description="Txodds Market Bot for Quadratic Market")
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run a single pass and exit (useful for cron jobs)",
    )
    args = parser.parse_args()
    asyncio.run(main(once=args.once))
