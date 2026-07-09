"""
Low-level Solana helpers: keypair loading, PDA derivation, and
wrappers around every on-chain instruction the bot needs.

All public functions are async and return the transaction signature string.
"""

import json
from pathlib import Path

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
SEED_LP_MINT = b"lp_mint"
SEED_MARKET = b"market"
SEED_MARKET_GROUP = b"market_group"
SEED_EPOCH = b"epoch"


def load_keypair(path: Path) -> Keypair:
    """Load a Solana keypair from a JSON file (array of 64 bytes)."""
    raw = json.loads(path.read_text())
    return Keypair.from_bytes(bytes(raw))


def normalize_idl_account_item(item: dict) -> dict:
    """Normalize account items from Anchor's newer IDL shape to AnchorPy-compatible shape."""
    if not isinstance(item, dict):
        return item

    if "accounts" in item:
        return {
            **item,
            "accounts": [normalize_idl_account_item(acc) for acc in item["accounts"]],
        }

    normalized = dict(item)
    if "writable" in normalized:
        normalized["is_mut"] = normalized.pop("writable")
    if "signer" in normalized:
        normalized["is_signer"] = normalized.pop("signer")
    if "optional" in normalized and "is_optional" not in normalized:
        normalized["is_optional"] = normalized.pop("optional")

    normalized.setdefault("is_mut", False)
    normalized.setdefault("is_signer", False)
    normalized.setdefault("is_optional", None)
    normalized.setdefault("docs", None)
    normalized.setdefault("relations", [])

    if "accounts" in normalized:
        normalized["accounts"] = [normalize_idl_account_item(acc) for acc in normalized["accounts"]]

    return normalized


def normalize_idl_json(raw_idl: dict) -> dict:
    normalized = dict(raw_idl)

    if "instructions" in raw_idl:
        normalized["instructions"] = [
            {
                **instr,
                "accounts": [normalize_idl_account_item(acc) for acc in instr.get("accounts", [])],
            }
            for instr in raw_idl["instructions"]
        ]

    if "accounts" in raw_idl:
        normalized["accounts"] = [normalize_idl_account_item(acc) for acc in raw_idl["accounts"]]

    if "state" in raw_idl and isinstance(raw_idl["state"], dict):
        state = dict(raw_idl["state"])
        if "methods" in state:
            state["methods"] = [
                {
                    **method,
                    "accounts": [normalize_idl_account_item(acc) for acc in method.get("accounts", [])],
                }
                for method in state["methods"]
            ]
        normalized["state"] = state

    return normalized


def load_idl(path: Path) -> Idl:
    raw_idl = json.loads(path.read_text())
    normalized = normalize_idl_json(raw_idl)
    return Idl.from_json(json.dumps(normalized))


def global_config_pda(program_id: Pubkey) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address([SEED_GLOBAL_CONFIG], program_id)


def treasury_pda(program_id: Pubkey) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address([SEED_TREASURY], program_id)


def lp_mint_pda(program_id: Pubkey) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address([SEED_LP_MINT], program_id)


def market_pda(program_id: Pubkey, market_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_MARKET, market_id.to_bytes(8, "little")],
        program_id,
    )


def market_group_pda(program_id: Pubkey, group_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_MARKET_GROUP, group_id.to_bytes(8, "little")],
        program_id,
    )


def epoch_pda(program_id: Pubkey, epoch_id: int) -> tuple[Pubkey, int]:
    return Pubkey.find_program_address(
        [SEED_EPOCH, epoch_id.to_bytes(8, "little")],
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
        self.lp_mint, _ = lp_mint_pda(self.program_id)

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
        idl = load_idl(idl_path)
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

    async def fetch_market_group(self, group_id: int) -> dict:
        pda, _ = market_group_pda(self.program_id, group_id)
        return await self.program.account["MarketGroup"].fetch(pda)

    async def fetch_epoch(self, epoch_id: int) -> dict:
        pda, _ = epoch_pda(self.program_id, epoch_id)
        return await self.program.account["Epoch"].fetch(pda)

    async def next_market_id(self) -> int:
        cfg = await self.fetch_global_config()
        return int(cfg.next_market_id)

    # ── Market Group operations ─────────────────────────────────────────────────

    async def create_market_group(self, group_id: int, description: str = "") -> tuple[int, str]:
        """
        Create a new market group for organizing related markets (e.g., same fixture).
        
        Args:
            group_id: Unique identifier for the group
            description: Optional description
            
        Returns:
            (group_id, tx_signature)
        """
        mg_pda, _ = market_group_pda(self.program_id, group_id)

        sig = await self.program.rpc["create_market_group"](
            group_id,
            description,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market_group": mg_pda,
                    "authority": self.operator_kp.pubkey(),
                    "system_program": SYS_PROGRAM_ID,
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("create_market_group", group_id=group_id, sig=str(sig))
        return group_id, str(sig)

    async def add_market_to_group(
        self,
        group_id: int,
        market_id: int,
        outcome_ids: list[int],
    ) -> str:
        """
        Add a market to an existing market group.
        
        Args:
            group_id: The market group ID
            market_id: The market to add
            outcome_ids: Which outcomes are part of this market group
            
        Returns:
            tx_signature
        """
        mg_pda, _ = market_group_pda(self.program_id, group_id)
        mkt_pda, _ = market_pda(self.program_id, market_id)

        sig = await self.program.rpc["add_market_to_group"](
            group_id,
            market_id,
            outcome_ids,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market_group": mg_pda,
                    "market": mkt_pda,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("add_market_to_group", group_id=group_id, market_id=market_id, sig=str(sig))
        return str(sig)

    async def activate_seeded_market(
        self,
        group_id: int,
        market_id: int,
        seed_amount: int,
        outcome_id: int,
    ) -> str:
        """
        Seed a market with initial liquidity for a specific outcome.
        
        Args:
            group_id: The market group ID
            market_id: The market to seed
            seed_amount: Amount to seed (in base mint units)
            outcome_id: Which outcome to seed
            
        Returns:
            tx_signature
        """
        mg_pda, _ = market_group_pda(self.program_id, group_id)
        mkt_pda, _ = market_pda(self.program_id, market_id)

        sig = await self.program.rpc["activate_seeded_market"](
            group_id,
            market_id,
            seed_amount,
            outcome_id,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market_group": mg_pda,
                    "market": mkt_pda,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("activate_seeded_market", group_id=group_id, market_id=market_id, 
                 seed_amount=seed_amount, outcome_id=outcome_id, sig=str(sig))
        return str(sig)

    # ── Market operations ─────────────────────────────────────────────────────

    async def create_market(
        self,
        start_time: int,
        num_outcomes: int,
        title: str,
        description: str,
        category: int = 0,
        lmsr_b_override: int | None = None,
        initial_q_values: list[int] | None = None,
    ) -> tuple[int, str]:
        """
        Create a market with optional initial q_values for odds seeding.
        
        Args:
            start_time: Unix timestamp when market starts
            num_outcomes: 2 or 3
            title: Market title (e.g., "Arsenal vs Liverpool - Match Result")
            description: Market description
            category: Market category (0=3-way, 1=BTTS, 2=Totals, etc.)
            lmsr_b_override: Override liquidity parameter B (optional)
            initial_q_values: Seed q_values from API odds (optional)
                              Must have length >= num_outcomes
        
        Returns:
            (market_id, tx_signature)
        """
        mid = await self.next_market_id()
        mkt_pda, _ = market_pda(self.program_id, mid)
        cfg = await self.fetch_global_config()

        sig = await self.program.rpc["create_market"](
            start_time,
            num_outcomes,
            title,
            description,
            category,
            lmsr_b_override,
            initial_q_values,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "authority": self.operator_kp.pubkey(),
                    "system_program": SYS_PROGRAM_ID,
                    "rent": Pubkey.from_string("SysvarRent111111111111111111111111111111111"),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info(
            "create_market",
            market_id=mid,
            title=title[:50],
            category=category,
            num_outcomes=num_outcomes,
            has_q_values=initial_q_values is not None,
            sig=str(sig),
        )
        return mid, str(sig)

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

    async def resume_market(self, market_id: int) -> str:
        """
        Resume a suspended market (if betting should reopen).
        """
        mkt_pda, _ = market_pda(self.program_id, market_id)
        sig = await self.program.rpc["resume_market"](
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("resume_market", market_id=market_id, sig=str(sig))
        return str(sig)

    async def void_market(self, market_id: int) -> str:
        """
        Void a market (admin action) — refunds all positions.
        """
        mkt_pda, _ = market_pda(self.program_id, market_id)
        sig = await self.program.rpc["void_market"](
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "authority": self.oracle_kp.pubkey(),
                },
                signers=[self.oracle_kp],
            ),
        )
        log.info("void_market", market_id=market_id, sig=str(sig))
        return str(sig)

    async def void_if_expired(self, market_id: int) -> str:
        """Void a market that exceeded the settlement deadline."""
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

    # ── Settlement ────────────────────────────────────────────────────────────

    async def admin_override(self, market_id: int, winning_outcome: int) -> str:
        """
        Admin override to set the winning outcome directly.
        Used when the oracle is unavailable or for testing.
        """
        mkt_pda, _ = market_pda(self.program_id, market_id)
        sig = await self.program.rpc["admin_override"](
            market_id,
            winning_outcome,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "authority": self.oracle_kp.pubkey(),
                },
                signers=[self.oracle_kp],
            ),
        )
        log.info("admin_override", market_id=market_id, outcome=winning_outcome, sig=str(sig))
        return str(sig)

    async def finalize_result(self, market_id: int) -> str:
        """
        Finalize after the challenge window. Callable by anyone — bot uses operator key.
        """
        mkt_pda, _ = market_pda(self.program_id, market_id)

        sig = await self.program.rpc["finalize_result"](
            market_id,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "market": mkt_pda,
                    "caller": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("finalize_result", market_id=market_id, sig=str(sig))
        return str(sig)

    # ── Liquidity operations ──────────────────────────────────────────────────

    async def add_liquidity(self, amount: int) -> str:
        """
        Add liquidity to the protocol.
        
        Args:
            amount: Amount of base tokens to add
            
        Returns:
            tx_signature
        """
        sig = await self.program.rpc["add_liquidity"](
            amount,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "treasury": self.treasury,
                    "authority": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("add_liquidity", amount=amount, sig=str(sig))
        return str(sig)

    # ── Config operations ─────────────────────────────────────────────────────

    async def update_config(
        self,
        max_market_exposure: int | None = None,
        challenge_window_seconds: int | None = None,
        settlement_deadline_seconds: int | None = None,
        lmsr_default_b: int | None = None,
        epoch_duration_seconds: int | None = None,
        withdrawal_cooldown_seconds: int | None = None,
        max_single_bet: int | None = None,
        min_outcome_price_bps: int | None = None,
        buy_fee_bps: int | None = None,
        oracle_pubkey: bytes | None = None,
        cash_out_margin_bps: int | None = None,
    ) -> str:
        """
        Update global configuration parameters.
        """
        sig = await self.program.rpc["update_config"](
            max_market_exposure,
            challenge_window_seconds,
            settlement_deadline_seconds,
            lmsr_default_b,
            epoch_duration_seconds,
            withdrawal_cooldown_seconds,
            max_single_bet,
            min_outcome_price_bps,
            buy_fee_bps,
            oracle_pubkey,
            cash_out_margin_bps,
            ctx=Context(
                accounts={
                    "global_config": self.global_config,
                    "admin": self.operator_kp.pubkey(),
                },
                signers=[self.operator_kp],
            ),
        )
        log.info("update_config", sig=str(sig))
        return str(sig)

    # ── Market info helpers ───────────────────────────────────────────────────

    def get_market_status(self, market_data: dict) -> str:
        """Get human-readable market status."""
        status = market_data.get("status", {})
        if "open" in status:
            return "open"
        elif "suspended" in status:
            return "suspended"
        elif "proposed" in status:
            return "proposed"
        elif "settled" in status:
            return "settled"
        elif "voided" in status:
            return "voided"
        return "unknown"

    def get_market_info(self, market_data: dict) -> dict:
        """Extract key info from market data."""
        return {
            "status": self.get_market_status(market_data),
            "market_id": int(market_data.get("market_id", 0)),
            "start_time": int(market_data.get("start_time", 0)),
            "num_outcomes": int(market_data.get("num_outcomes", 0)),
            "title": market_data.get("title", ""),
            "description": market_data.get("description", ""),
            "winning_outcome": market_data.get("winning_outcome"),
            "total_exposure": market_data.get("total_exposure", 0),
            "pool_size": market_data.get("pool_size", 0),
        }
