"""Bot configuration loaded from environment variables."""

import os
from pathlib import Path
from dotenv import load_dotenv

load_dotenv()


def _require(key: str) -> str:
    val = os.getenv(key)
    if not val:
        raise RuntimeError(f"Required env var {key!r} is not set. Copy .env.example to .env and fill it in.")
    return val


def _require_default(key: str, default: str) -> str:
    """Get env var or use default."""
    return os.getenv(key, default)


def _keypair_path(key: str) -> Path:
    raw = _require(key)
    p = Path(raw).expanduser()
    if not p.exists():
        raise RuntimeError(f"Keypair file {p} (from {key}) does not exist.")
    return p


def _keypair_path_optional(key: str) -> Path | None:
    """Get keypair path or return None if not set."""
    raw = os.getenv(key)
    if not raw:
        return None
    p = Path(raw).expanduser()
    if not p.exists():
        return None
    return p


# ─── Solana Configuration ──────────────────────────────────────────────────────

RPC_URL: str = os.getenv("RPC_URL", "https://api.devnet.solana.com")
PROGRAM_ID: str = _require("PROGRAM_ID")
BASE_MINT: str = _require("BASE_MINT")

OPERATOR_KEYPAIR_PATH: Path = _keypair_path("OPERATOR_KEYPAIR_PATH")
ORACLE_KEYPAIR_PATH: Path = _keypair_path("ORACLE_KEYPAIR_PATH")

# ─── Sports API Configuration (Optional) ──────────────────────────────────────────

# The-Odds-API (primary)
ODDS_API_KEY: str = os.getenv("ODDS_API_KEY", "")

# TXOdds API (optional - for more accurate odds)
TXODDS_API_TOKEN: str = os.getenv("TXODDS_API_TOKEN", "")

# Football leagues to track (comma-separated)
# Supported: soccer_epl, soccer_uefa_champs_league, soccer_uefa_europa_league,
# soccer_spain_la_liga, soccer_germany_bundesliga, soccer_italy_serie_a,
# soccer_france_ligue_one
SPORTS: list[str] = [s.strip() for s in os.getenv("SPORTS", "soccer_epl").split(",") if s.strip()]

# ─── Bot Timing Configuration ───────────────────────────────────────────────────

# How far ahead to look for upcoming matches (seconds)
MARKET_LOOKAHEAD_SECONDS: int = int(os.getenv("MARKET_LOOKAHEAD_SECONDS", "86400"))  # 24 hours

# How long after match start to wait before settling (seconds)
# This should be based on expected match duration + extra time
RESULT_DELAY_SECONDS: int = int(os.getenv("RESULT_DELAY_SECONDS", "7200"))  # 2 hours (for testing)
# Production setting: 5400 (90 min match + 30 min extra time + 30 min buffer)

# How often to poll for updates (seconds)
POLL_INTERVAL_SECONDS: int = int(os.getenv("POLL_INTERVAL_SECONDS", "300"))  # 5 minutes

# Market creation lookahead - how early to create markets before match start
MARKET_CREATION_LEAD_TIME: int = int(os.getenv("MARKET_CREATION_LEAD_TIME", "172800"))  # 48 hours

# Settlement check interval - how often to check if markets are ready to settle
SETTLEMENT_CHECK_INTERVAL: int = int(os.getenv("SETTLEMENT_CHECK_INTERVAL", "60"))  # 1 minute

# ─── Market Tracking Configuration ───────────────────────────────────────────────────

# Track and sync markets from on-chain state
SYNC_ONCHAIN_MARKETS: bool = os.getenv("SYNC_ONCHAIN_MARKETS", "true").lower() == "true"

# Maximum markets to track per run
MAX_MARKETS_PER_RUN: int = int(os.getenv("MAX_MARKETS_PER_RUN", "20"))

# Market stage timeouts (seconds)
SUSPEND_GRACE_PERIOD: int = int(os.getenv("SUSPEND_GRACE_PERIOD", "60"))  # Grace period after start_time
PROPOSAL_TIMEOUT: int = int(os.getenv("PROPOSAL_TIMEOUT", "14400"))  # 4 hours to propose result
FINALIZATION_GRACE_PERIOD: int = int(os.getenv("FINALIZATION_GRACE_PERIOD", "60"))  # Grace period after challenge window
VOID_DEADLINE: int = int(os.getenv("VOID_DEADLINE", "28800"))  # 8 hours to void if no settlement

# ─── LMSR Configuration ───────────────────────────────────────────────────

# Default B parameter for LMSR (in lamports)
# 100_000_000 lamports = 0.1 SOL/USDC (adjust based on your token decimals)
LMSR_B_DEFAULT: int = int(os.getenv("LMSR_B_DEFAULT", "100_000_000"))

# Minimum B parameter
LMSR_B_MIN: int = int(os.getenv("LMSR_B_MIN", "10_000_000"))

# Maximum B parameter
LMSR_B_MAX: int = int(os.getenv("LMSR_B_MAX", "1_000_000_000"))

# ─── Logging ──────────────────────────────────────────────────────────────────

LOG_LEVEL: str = os.getenv("LOG_LEVEL", "INFO").upper()

# ─── State File ──────────────────────────────────────────────────────────────────

STATE_FILE: Path = Path(os.getenv("STATE_FILE", "")).expanduser()
if not STATE_FILE or STATE_FILE == Path(""):
    STATE_FILE = Path(__file__).parent / "bot_state.json"
