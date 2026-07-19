// Quadratic Market Protocol Deployment Script
// This script initializes the protocol after deployment

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { QuadraticMarket } from "../target/types/quadratic_market";

// Program ID from Anchor.toml
const PROGRAM_ID = new PublicKey("4wKXu91KW6EBiecjUUYupQHjab6AULrGCm6hNrWbAvaA");

async function main() {
  // Use AnchorProvider.env() which reads from Anchor.toml
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.QuadraticMarket as Program<QuadraticMarket>;

  const admin = provider.wallet.publicKey;
  console.log("Deploying Quadratic Market Protocol...");
  console.log("Provider cluster:", provider.connection.rpcEndpoint);
  console.log("Admin:", admin.toString());

  // Derive PDAs
  const [globalConfigPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("global_config")],
    program.programId
  );
  const [lpMintPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("lp_mint")],
    program.programId
  );
  const [treasuryPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("treasury")],
    program.programId
  );

  console.log("Global Config PDA:", globalConfigPda.toString());
  console.log("LP Mint PDA:", lpMintPda.toString());
  console.log("Treasury PDA:", treasuryPda.toString());

  // Check if already initialized
  try {
    const existingConfig = await program.account.globalConfig.fetch(globalConfigPda);
    console.log("Protocol already initialized at:", globalConfigPda.toString());
    console.log("Skipping initialization...");
    return;
  } catch (e) {
    // Not initialized, proceed
    console.log("Protocol not initialized, proceeding with deployment...");
  }

  // For devnet, we use a custom test mint
  const baseMint = new PublicKey("A8YVMvoxYfJzqqXiq7PDtCkjFp2iWDvn1MnHyzbmUHDx");
  
  // Generate oracle keypair for testing
  const oracleKeypair = Keypair.generate();

  console.log("Base Mint:", baseMint.toString());
  console.log("Oracle PublicKey:", oracleKeypair.publicKey.toString());

  try {
    // 1. Initialize the protocol
    console.log("\n1. Initializing protocol...");
    const initializeTx = await program.methods
      .initialize(
        Array.from(oracleKeypair.publicKey.toBytes()),
        new anchor.BN(1_000_000_000) // max_market_exposure: 1000 USDC (6 decimals)
      )
      .accountsStrict({
        globalConfig: globalConfigPda,
        lpMint: lpMintPda,
        treasury: treasuryPda,
        baseMint: baseMint,
        admin: admin,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .signers([])
      .rpc();

    console.log("Protocol initialized!");
    console.log("Transaction:", initializeTx);

  // 2. Initialize the first epoch
  console.log("\n2. Initializing first epoch...");
    const [epochPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("epoch"), new anchor.BN(0).toArrayLike(Buffer, "le", 8)],
      program.programId
    );
    console.log("Epoch 0 PDA:", epochPda.toString());

    const epochTx = await program.methods
      .initEpoch()
      .accountsStrict({
        globalConfig: globalConfigPda,
        epoch: epochPda,
        authority: admin,
        systemProgram: SystemProgram.programId,
      })
      .signers([])
      .rpc();

    console.log("Epoch 0 initialized!");
    console.log("Transaction:", epochTx);

    console.log("\n=== Deployment Complete ===");
    console.log("Global Config:", globalConfigPda.toString());
    console.log("Next steps:");
    console.log("1. Add settlement operators with addSettlementOperator");
    console.log("2. Add market operators with addOperator");
    console.log("3. Add initial liquidity with addLiquidity");
    console.log("4. Create markets with createMarket");

  } catch (error) {
    console.error("Deployment failed:", error);
    throw error;
  }
}

// Run main if executed directly
main().catch(console.error);

// Export for anchor deploy
module.exports = async function (provider: anchor.AnchorProvider) {
  anchor.setProvider(provider);
  await main();
};
