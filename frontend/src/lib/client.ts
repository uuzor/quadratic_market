// Solana client utilities
// These functions avoid executing Solana SDK code at module level to prevent SSG errors

// Program ID as constant string
export const PROGRAM_ID_STRING = "4wKXu91KW6EBiecjUUYupQHjab6AULrGCm6hNrWbAvaA";

// PDA seed constants as strings
export const GLOBAL_CONFIG_SEED = "global_config";
export const EPOCH_SEED = "epoch";
export const MARKET_SEED = "market";
export const MARKET_GROUP_SEED = "market_group";

// Import these lazily to avoid SSG issues
let _PublicKey: any = null;
let _PROGRAM_ID: any = null;

function getPublicKey() {
  if (!_PublicKey) {
    // @ts-ignore
    _PublicKey = require("@solana/web3.js").PublicKey;
  }
  return _PublicKey;
}

function getProgramId() {
  if (!_PROGRAM_ID) {
    const PK = getPublicKey();
    _PROGRAM_ID = new PK(PROGRAM_ID_STRING);
  }
  return _PROGRAM_ID;
}

// Get program address as string
export function getProgramAddress(): string {
  return PROGRAM_ID_STRING;
}

// Derive global config PDA
export function deriveGlobalConfig(): any {
  const PK = getPublicKey();
  const PROGRAM_ID = getProgramId();
  return PK.findProgramAddressSync(
    [Buffer.from(GLOBAL_CONFIG_SEED)],
    PROGRAM_ID
  )[0];
}

// Derive market PDA
export function deriveMarket(marketId: number): any {
  const PK = getPublicKey();
  const PROGRAM_ID = getProgramId();
  const marketIdBuffer = Buffer.alloc(8);
  marketIdBuffer.writeBigUInt64LE(BigInt(marketId));
  return PK.findProgramAddressSync(
    [Buffer.from(MARKET_SEED), marketIdBuffer],
    PROGRAM_ID
  )[0];
}

// Derive epoch PDA
export function deriveEpoch(epochId: number): any {
  const PK = getPublicKey();
  const PROGRAM_ID = getProgramId();
  const epochIdBuffer = Buffer.alloc(8);
  epochIdBuffer.writeBigUInt64LE(BigInt(epochId));
  return PK.findProgramAddressSync(
    [Buffer.from(EPOCH_SEED), epochIdBuffer],
    PROGRAM_ID
  )[0];
}

// Connection URL
export const connectionUrl = "https://api.devnet.solana.com";

// Token program constant
export const TOKEN_PROGRAM_STRING = "TokenkegQfeZyiNwAjbOdcfLS7PLN3NBuHTL8J5Cw4";
export const ASSOCIATED_TOKEN_PROGRAM_STRING = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

export function getTokenProgram(): any {
  const PK = getPublicKey();
  return new PK(TOKEN_PROGRAM_STRING);
}

export function getAssociatedTokenProgram(): any {
  const PK = getPublicKey();
  return new PK(ASSOCIATED_TOKEN_PROGRAM_STRING);
}

// For backward compatibility
export const TOKEN_PROGRAM = TOKEN_PROGRAM_STRING;
export const ASSOCIATED_TOKEN_PROGRAM = ASSOCIATED_TOKEN_PROGRAM_STRING;
export const SYSTEM_PROGRAM = "11111111111111111111111111111111";
