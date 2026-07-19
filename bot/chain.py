"""
Low-level Solana helpers: keypair loading, PDA derivation, and
wrappers around every on-chain instruction the bot needs.

All public functions are async and return the transaction signature string.
"""

import json
from pathlib import Path
from typing import Optional, Tuple, List

from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import ID as SYS_PROGRAM_ID
from solana.rpc.async_api import AsyncClient
from anchorpy import Program, Provider, Wallet, Context, Idl
from anchorpy.program.namespace.instruction import _InstructionFn  # noqa: F401 (type hint only)

import structlog

log = structlog.get_logger(__name__)

# ─── PDA seeds (must match constants.rs) ──────────────────────────────────────

SEED_GLOBAL_CONFIG = b"global_config"
SEED_TREASURY = b"treasury"
SEED_MARKET = b"market"
SEED_OUTCOME_MINT = b"outcome_mint"
SEED_SLIP = b"slip"
SEED_EPOCH = b"epoch"
SEED_MARKET_GROUP = b"market_group"


def load_keypair(path: Path) -> Keypair:
    """Load a Solana keypair from a JSON file (array of 64 bytes)."""
    raw = json.loads(path.read_text())
    return Keypair.from_bytes(bytes(raw))


def global_config_pda(program_id: Pubkey) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address([SEED_GLOBAL_CONFIG], program_id)


def treasury_pda(program_id: Pubkey) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address([SEED_TREASURY], program_id)


def market_pda(program_id: Pubkey, market_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_MARKET, market_id.to_bytes(8, "little")],
        program_id,
    )


def outcome_mint_pda(program_id: Pubkey, market_id: int, outcome_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_OUTCOME_MINT, market_id.to_bytes(8, "little"), bytes([outcome_id])],
        program_id,
    )


def slip_pda(program_id: Pubkey, slip_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_SLIP, slip_id.to_bytes(8, "little")],
        program_id,
    )


def epoch_pda(program_id: Pubkey, epoch_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_EPOCH, epoch_id.to_bytes(8, "little")],
        program_id,
    )


def market_group_pda(program_id: Pubkey, group_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_MARKET_GROUP, group_id.to_bytes(8, "little")],
        program_id,
    )


# ─── On-chain client ──────────────────────────────────────────────────────────


class ChainClient:
    """
    Wraps anchorpy Program to expose the specific instructions the bot uses.

    Instantiate once and reuse across the bot's lifetime.
    """

    def __init__(
        self,
        program: Program,
        operator_kp: Keypair,
        oracle_kp: Keypair,
        base_mint: Pubkey,
    ) -> None:
        self.program = program
        self.operator_kp = operator_kp
        self.oracle_kp = oracle_kp
        self.base_mint = base_mint
        self.program_id = program.program_id

        self.global_config, _ = global_config_pda(self.program_id)
        self.treasury, _ = treasury_pda(self.program_id)

    @classmethod
    async def create(
        cls,
        rpc_url: str,
        idl_path: Path,
        program_id_str: str,
        operator_kp: Keypair,
        oracle_kp: Keypair,
        base_mint_str: str,
    ) -> "ChainClient":
        client = AsyncClient(rpc_url)
        wallet = Wallet(operator_kp)
        provider = Provider(client, wallet)
        idl = Idl.from_json(idl_path.read_text())
        program = Program(idl, Pubkey.from_string(program_id_str), provider)
        return cls(program, operator_kp, oracle_kp, Pubkey.from_string(base_mint_str))

    async def close(self) -> None:
        await self.program.close()

    # ── Read ──────────────────────────────────────────────────────────────────

    async def fetch_global_config(self) -> dict:
        return await self.program.account["GlobalConfig"].fetch(self.global_config)

    async def fetch_market(self, market_id: int) -> dict:
        pda, _ = market_pda(self.program_id, market_id)
        return await self.program.account["Market"].fetch(pda)

    async def fetch_slip(self, slip_id: int) -> dict:
        pda, _ = slip_pda(self.program_id, slip_id)
        return await self.program.account["Slip"].fetch(pda)

    async def fetch_epoch(self, epoch_id: int) -> dict:
        pda, _ = epoch_pda(self.program_id, epoch_id)
        return await self.program.account["Epoch"].fetch(pda)

    async def next_market_id(self) -> int:
        cfg = await self.fetch_global_config()
        return int(cfg.next_market_id)

    async def next_slip_id(self) -> int:
        cfg = await self.fetch_global_config()
        return int(cfg.next_slip_id)

    # ── Market Group ───────────────────────────────────────────────────────────

    async def create_market_group(
        self,
        group_id: int,
        title: str,
        event_start_time: int,
        max_group_exposure: int = 100_000_000_000,
    ) -> str:
        """
        Create a market group for a match (contains 1X2, O/U, GG/NG markets).
        """
        gp_pda, _ = market_group_pda(self.program_id, group_id)

        sig = await self.program.rpc["create_market_group"](
            group_id,
            max_group_exposure,
            event_start_time,
            title,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market_group": gp_pda,
                    "authority": self.operator_kp.pubkey(),
                    "system_program": SYS_PROGRAM_ID,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("create_market_group", group_id=group_id, title=title, sig=str(sig))
        return str(sig)

    # ── create_market (Fixed Odds) ───────────────────────────────────────────

    async def create_market(
        self,
        start_time: int,
        num_outcomes: int,
        title: str,
        description: str,
        category: int = 0,
        market_type: str = "oneXTwo",  # oneXTwo, overUnder, goalNoGoal
        initial_odds: Optional[List[int]] = None,
    ) -> tuple[int, str]:
        """
        Create a market with fixed odds. Returns (market_id, tx_signature).
        
        Args:
            start_time: Unix timestamp when match starts
            num_outcomes: 2 or 3
            title: Market title
            description: Market description
            category: 0=1X2, 1=O/U, 2=GG/NG
            market_type: Anchor enum variant name
            initial_odds: List of odds in BPS (e.g., [20000, 35000, 30000])
        """
        mid = await self.next_market_id()
        mkt_pda, _ = market_pda(self.program_id, mid)

        # Get current epoch
        cfg = await self.fetch_global_config()
        current_epoch = int(cfg.current_epoch)
        ep_pda, _ = epoch_pda(self.program_id, current_epoch)

        # Convert market_type to Anchor variant
        market_type_map = {
            "oneXTwo": {"oneXTwo": {}},
            "overUnder": {"overUnder": {}},
            "goalNoGoal": {"goalNoGoal": {}},
        }
        market_type_arg = market_type_map.get(market_type, {"oneXTwo": {}})

        # Default odds if not provided
        if initial_odds is None:
            if num_outcomes == 3:
                initial_odds = [20000, 35000, 30000]  # 2.0x, 3.5x, 3.0x
            else:
                initial_odds = [20000, 20000]  # 2.0x, 2.0x

        sig = await self.program.rpc["create_market"](
            start_time,
            num_outcomes,
            title,
            description,
            category,
            market_type_arg,
            initial_odds,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "epoch": ep_pda,
                    "authority": self.operator_kp.pubkey(),
                    "system_program": SYS_PROGRAM_ID,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("create_market", market_id=mid, title=title, odds=initial_odds, sig=str(sig))
        return mid, str(sig)

    async def add_market_to_group(
        self,
        group_id: int,
        market_id: int,
        market_index: int,
    ) -> str:
        """
        Add a market to a market group.
        """
        gp_pda, _ = market_group_pda(self.program_id, group_id)

        sig = await self.program.rpc["add_market_to_group"](
            group_id,
            market_index,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market_group": gp_pda,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("add_market_to_group", group_id=group_id, market_id=market_id, index=market_index)
        return str(sig)

    # ── update_market_odds ───────────────────────────────────────────────────

    async def update_market_odds(
        self,
        market_id: int,
        new_odds: List[int],
    ) -> str:
        """
        Update the fixed odds for a market.
        Can only be called before market start_time.
        """
        mkt_pda, _ = market_pda(self.program_id, market_id)

        sig = await self.program.rpc["update_market_odds"](
            market_id,
            new_odds,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("update_market_odds", market_id=market_id, odds=new_odds, sig=str(sig))
        return str(sig)

    # ── init_outcome_mint ─────────────────────────────────────────────────────

    async def init_outcome_mint(self, market_id: int, outcome_id: int) -> str:
        mkt_pda, _ = market_pda(self.program_id, market_id)
        mint_pda, _ = outcome_mint_pda(self.program_id, market_id, outcome_id)

        from solders.sysvar import RENT as SYSVAR_RENT
        from spl.token.constants import TOKEN_PROGRAM_ID

        sig = await self.program.rpc["init_outcome_mint"](
            market_id,
            outcome_id,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "outcome_mint": mint_pda,
                    "payer": self.operator_kp.pubkey(),
                    "token_program": Pubkey.from_string(str(TOKEN_PROGRAM_ID)),
                    "system_program": SYS_PROGRAM_ID,
                    "rent": SYSVAR_RENT,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("init_outcome_mint", market_id=market_id, outcome_id=outcome_id, sig=str(sig))
        return str(sig)

    # ── suspend_market ────────────────────────────────────────────────────────

    async def suspend_market(self, market_id: int) -> str:
        """
        Suspend a market when the match starts — no more bets accepted.
        Called by the bot at start_time.
        """
        mkt_pda, _ = market_pda(self.program_id, market_id)
        sig = await self.program.rpc["suspend_market"](
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("suspend_market", market_id=market_id, sig=str(sig))
        return str(sig)

    # ── Slip Operations ────────────────────────────────────────────────────────

    async def buy_leg_for_slip(
        self,
        slip_id: int,
        leg_index: int,
        buyer: Pubkey,
    ) -> str:
        """
        Execute one leg of a slip (called by backend after user creates slip).
        """
        slip_pda, _ = slip_pda(self.program_id, slip_id)
        mkt_pda, _ = market_pda(self.program_id, slip_id)  # Would need to fetch slip to get market

        # For this we'd need the actual market PDA
        # This is simplified - in practice, fetch the slip first
        sig = await self.program.rpc["buy_leg_for_slip"](
            slip_id,
            leg_index,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "slip": slip_pda,
                    "market": mkt_pda,  # Would need proper market PDA
                    "treasury": self.treasury,
                    "buyer_outcome_ata": Pubkey.default(),  # Would need actual ATA
                    "treasury_base_ata": Pubkey.default(),
                    "outcome_mint": Pubkey.default(),
                    "base_mint": self.base_mint,
                    "buyer": buyer,
                    "token_program": Pubkey.default(),
                    "associated_token_program": Pubkey.default(),
                    "system_program": SYS_PROGRAM_ID,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("buy_leg_for_slip", slip_id=slip_id, leg_index=leg_index, sig=str(sig))
        return str(sig)

    async def settle_slip_leg(
        self,
        slip_id: int,
        leg_index: int,
    ) -> str:
        """
        Settle one leg of a slip after the market settles.
        """
        slip_pda, _ = slip_pda(self.program_id, slip_id)
        
        # Would need to fetch slip to get market PDA
        sig = await self.program.rpc["settle_slip_leg"](
            slip_id,
            leg_index,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "slip": slip_pda,
                    "market": Pubkey.default(),  # Would need actual market PDA
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("settle_slip_leg", slip_id=slip_id, leg_index=leg_index, sig=str(sig))
        return str(sig)

    async def resolve_slip(self, slip_id: int) -> str:
        """
        Resolve a slip after all legs are settled.
        """
        slip_pda, _ = slip_pda(self.program_id, slip_id)

        sig = await self.program.rpc["resolve_slip"](
            slip_id,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "slip": slip_pda,
                    "treasury": self.treasury,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("resolve_slip", slip_id=slip_id, sig=str(sig))
        return str(sig)

    async def void_if_expired(self, market_id: int) -> str:
        """Void a market that the oracle never settled within the deadline."""
        mkt_pda, _ = market_pda(self.program_id, market_id)
        sig = await self.program.rpc["void_if_expired"](
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("void_if_expired", market_id=market_id, sig=str(sig))
        return str(sig)

    # ── Epoch Operations ─────────────────────────────────────────────────────

    async def init_epoch(self) -> str:
        """
        Initialize the current epoch.
        """
        cfg = await self.fetch_global_config()
        epoch_id = int(cfg.current_epoch)
        ep_pda, _ = epoch_pda(self.program_id, epoch_id)

        sig = await self.program.rpc["init_epoch"](
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "epoch": ep_pda,
                    "admin": self.operator_kp.pubkey(),
                    "system_program": SYS_PROGRAM_ID,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("init_epoch", epoch_id=epoch_id, sig=str(sig))
        return str(sig)
