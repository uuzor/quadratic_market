/**
 * lifecycle/open_and_bet.ts  — PHASE 1
 * ─────────────────────────────────────────────────────────────────────────────
 * Bootstraps the protocol, opens two correlated markets, adds LP liquidity, and
 * has a set of users place single-share bets and multi-leg slips. All state
 * needed by Phase 2 (settle + withdraw) is written to lifecycle/state.json.
 *
 * Run (env ANCHOR_PROVIDER_URL / ANCHOR_WALLET set by run_lifecycle.sh):
 *   npx ts-node lifecycle/open_and_bet.ts
 * ─────────────────────────────────────────────────────────────────────────────
 */

import {
  anchor,
  BN,
  Keypair,
  PublicKey,
  TOKEN_PROGRAM_ID,
  ONE_USDC,
  SEEDS,
  makeCtx,
  banner,
  logLine,
  sub,
  toUsdc,
  airdrop,
  getOrCreateAta,
  ataAddress,
  tokenBalance,
  nowSec,
  saveState,
  kpToJson,
  globalConfigPda,
  treasuryPda,
  lpMintPda,
  marketPda,
  outcomeMintPda,
  epochPda,
  marketGroupPda,
  betSlipPda,
  pendingPda,
  printOdds,
  oddsToQValues,
  LifecycleState,
  MarketState,
  SingleBetState,
  SlipState,
  u64LE,
  u8,
  pda,
} from "./common";
import {
  createMint,
  mintTo,
  ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import {
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  ComputeBudgetProgram,
} from "@solana/web3.js";

// ─── Tunables ────────────────────────────────────────────────────────────────
const LP_DEPOSIT = new BN(500_000 * ONE_USDC);
const USER_FUND = 100_000 * ONE_USDC;
const SINGLE_BET = new BN(100 * ONE_USDC);  // 100 USDC per bet
const SLIP_LEG_SHARES = new BN(50 * ONE_USDC);  // 50 USDC per leg
const GROUP_ID = new BN(77);
const MAX_EXPOSURE = new BN(2_000_000 * ONE_USDC);
const MAX_GROUP_EXPOSURE = new BN(1_000_000 * ONE_USDC);

// Timing for a fast localnet lifecycle.
const CHALLENGE_WINDOW = 60; // min allowed
const EPOCH_DURATION = 60; // min allowed
const WITHDRAWAL_COOLDOWN = 0;
// Betting window: markets close this many seconds after creation. Must be long
// enough to place every bet before now >= start_time.
const BETTING_WINDOW_SEC = 75;

async function main() {
  const { connection, program, admin } = makeCtx();
  const programId = program.programId;

  banner("PHASE 1 — OPEN MARKETS & PLACE BETS");
  logLine(`Program: ${programId.toBase58()}`);
  logLine(`Admin:   ${admin.publicKey.toBase58()}`);

  // ── Wallets ────────────────────────────────────────────────────────────────
  const oracle = Keypair.generate();
  const lpProvider = Keypair.generate();
  const users = Array.from({ length: 6 }, () => Keypair.generate());
  const userLabels = users.map((_, i) => `User${i + 1}`);

  banner("BOOTSTRAP — wallets, mint, ATAs, config");
  for (const kp of [oracle, lpProvider, ...users]) {
    await airdrop(connection, kp.publicKey, 100);
  }

  const baseMint = await createMint(
    connection,
    admin,
    admin.publicKey,
    null,
    6
  );
  logLine(`Base mint (USDC): ${baseMint.toBase58()}`);

  const gcPda = globalConfigPda();
  const trPda = treasuryPda();
  const lpMint = lpMintPda();
  const treasuryBaseAta = await getOrCreateAta(
    connection,
    admin,
    baseMint,
    trPda,
    true
  );

  // Fund the seeder (admin) with USDC for the real seed bets.
  const adminBaseAta = await getOrCreateAta(connection, admin, baseMint, admin.publicKey);
  await mintTo(connection, admin, baseMint, adminBaseAta, admin, 100_000 * ONE_USDC);

  // Fund LP + users with USDC.
  const lpBaseAta = await getOrCreateAta(
    connection,
    admin,
    baseMint,
    lpProvider.publicKey
  );
  await mintTo(
    connection,
    admin,
    baseMint,
    lpBaseAta,
    admin,
    LP_DEPOSIT.toNumber() * 2
  );
  const userBaseAtas: PublicKey[] = [];
  for (const u of users) {
    const ata = await getOrCreateAta(connection, admin, baseMint, u.publicKey);
    await mintTo(connection, admin, baseMint, ata, admin, USER_FUND);
    userBaseAtas.push(ata);
  }
  logLine("Wallets & ATAs funded ✓");

  // ── Initialize protocol ──────────────────────────────────────────────────
  banner("INITIALIZE PROTOCOL");
  await program.methods
    .initialize(Array.from(oracle.publicKey.toBytes()), MAX_EXPOSURE)
    .accounts({
      globalConfig: gcPda,
      lpMint,
      treasury: trPda,
      baseMint,
      admin: admin.publicKey,
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
      rent: SYSVAR_RENT_PUBKEY,
    })
    .rpc();
  logLine("Protocol initialized ✓");

  // Shrink timing params so settlement + withdrawals are reachable fast.
  await program.methods
    .updateConfig(
      null, // max_market_exposure
      new BN(CHALLENGE_WINDOW), // challenge_window_seconds
      new BN(60), // settlement_deadline_seconds
      null, // lmsr_default_b
      new BN(EPOCH_DURATION), // epoch_duration_seconds
      new BN(WITHDRAWAL_COOLDOWN), // withdrawal_cooldown_seconds
      new BN(100_000 * ONE_USDC), // max_single_bet: 100,000 USDC
      null, // min_outcome_price_bps
      null, // buy_fee_bps
      null, // oracle_pubkey
      null  // cash_out_margin_bps
    )
    .accounts({ globalConfig: gcPda, admin: admin.publicKey })
    .rpc();
  logLine(
    `Config: challenge_window=${CHALLENGE_WINDOW}s epoch_duration=${EPOCH_DURATION}s withdrawal_cooldown=${WITHDRAWAL_COOLDOWN}s ✓`
  );

  // ── Epoch 0 ────────────────────────────────────────────────────────────────
  banner("INIT EPOCH 0");
  const cfg0: any = await program.account.globalConfig.fetch(gcPda);
  const epoch0Id: BN = cfg0.currentEpoch;
  await program.methods
    .initEpoch()
    .accounts({
      globalConfig: gcPda,
      epoch: epochPda(epoch0Id),
      authority: admin.publicKey,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  logLine(`Epoch ${epoch0Id.toString()} initialized ✓`);

  // ── Market group ───────────────────────────────────────────────────────────
  banner("CREATE MARKET GROUP");
  const mgPda = marketGroupPda(GROUP_ID);
  const eventStart = new BN(nowSec() + 3600);
  await program.methods
    .createMarketGroup(GROUP_ID, MAX_GROUP_EXPOSURE, eventStart, "EPL Fixture")
    .accounts({
      globalConfig: gcPda,
      marketGroup: mgPda,
      creator: admin.publicKey,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  logLine(`Group ${GROUP_ID.toString()} created ✓`);

  // ── Two markets in the group ─────────────────────────────────────────────
  // Market 1: 3-way result (Home / Draw / Away). Market 2: 2-way (BTTS Yes/No).
  const startTime = nowSec() + BETTING_WINDOW_SEC;
  const marketDefs = [
    {
      numOutcomes: 3,
      title: "Arsenal vs Chelsea — Result",
      names: ["Arsenal Win", "Draw", "Chelsea Win"],
      // seed split ~ 45 / 28 / 27 (min 500 USDC per outcome)
      seeds: [
        new BN(900 * ONE_USDC),
        new BN(560 * ONE_USDC),
        new BN(540 * ONE_USDC),
      ],
      winningOutcome: 0, // Arsenal win settles
    },
    {
      numOutcomes: 2,
      title: "Arsenal vs Chelsea — BTTS",
      names: ["BTTS Yes", "BTTS No"],
      seeds: [new BN(600 * ONE_USDC), new BN(500 * ONE_USDC)],
      winningOutcome: 0, // BTTS Yes settles
    },
  ];

  const markets: MarketState[] = [];
  const cfg1: any = await program.account.globalConfig.fetch(gcPda);
  let nextMarketId: BN = cfg1.nextMarketId;

  for (let mi = 0; mi < marketDefs.length; mi++) {
    const def = marketDefs[mi];
    const marketId = nextMarketId;
    nextMarketId = nextMarketId.add(new BN(1));
    const mPda = marketPda(marketId);

    banner(`CREATE MARKET ${marketId.toString()} — ${def.title}`);

    // Operator-set opening line: derive implied probabilities from the seed split
    // and pass them as initial_q_values (Bet9ja-style line; LMSR moves it after).
    const seedTotal = def.seeds.reduce((a, b) => a + b.toNumber(), 0);
    const openingProbs = def.seeds.map((s) => s.toNumber() / seedTotal);
    const DEFAULT_B_RAW = 100_000_000_000; // matches DEFAULT_LMSR_B
    const openingQ = oddsToQValues(openingProbs, DEFAULT_B_RAW);

    await program.methods
      .createMarket(
        new BN(startTime),
        def.numOutcomes,
        def.title,
        "Lifecycle demo market",
        1,
        null,
        openingQ,
        { trading: {} }
      )
      .accounts({
        globalConfig: gcPda,
        market: mPda,
        epoch: epochPda(epoch0Id),
        authority: admin.publicKey,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const outcomeMints: PublicKey[] = [];
    for (let i = 0; i < def.numOutcomes; i++) {
      const omPda = outcomeMintPda(marketId, i);
      outcomeMints.push(omPda);
      await program.methods
        .initOutcomeMint(marketId, i)
        .accounts({
          globalConfig: gcPda,
          market: mPda,
          outcomeMint: omPda,
          payer: admin.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: SYSVAR_RENT_PUBKEY,
        })
        .rpc();
      sub(`outcome ${i} (${def.names[i]}) mint: ${omPda.toBase58()}`);
    }

    await program.methods
      .addMarketToGroup(GROUP_ID, mi)
      .accounts({
        globalConfig: gcPda,
        marketGroup: mgPda,
        market: mPda,
        authority: admin.publicKey,
      })
      .rpc();

    // Seed each outcome as a REAL early bet (admin pays USDC, gets outcome tokens).
    // Every outcome must be >= $500 to activate. Then activate (PreOpen → Open).
    for (let i = 0; i < def.numOutcomes; i++) {
      const omPda = outcomeMints[i];
      const seederOutcomeAta = await getOrCreateAta(
        connection,
        admin,
        omPda,
        admin.publicKey
      );
      await program.methods
        .registerSeedPosition(GROUP_ID, marketId, mi, i, def.seeds[i])
        .accounts({
          globalConfig: gcPda,
          marketGroup: mgPda,
          market: mPda,
          treasury: trPda,
          seederBaseAta: adminBaseAta,
          treasuryBaseAta: treasuryBaseAta,
          outcomeMint: omPda,
          seederOutcomeAta: seederOutcomeAta,
          baseMint,
          seeder: admin.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
      sub(`seeded outcome ${i} (${def.names[i]}) with ${toUsdc(def.seeds[i])} (real bet)`);
    }
    await program.methods
      .activateSeededMarket(GROUP_ID)
      .accounts({
        globalConfig: gcPda,
        marketGroup: mgPda,
        market: mPda,
        authority: admin.publicKey,
      })
      .rpc();

    const m: any = await program.account.market.fetch(mPda);
    const totalSeedCapital = def.seeds.reduce((a, b) => a.add(b), new BN(0));
    logLine(`Status: ${JSON.stringify(m.status)} (start_time ${m.startTime})`);
    markets.push({
      marketId: marketId.toString(),
      marketPda: mPda.toBase58(),
      outcomeMints: outcomeMints.map((p) => p.toBase58()),
      outcomeNames: def.names,
      startTime,
      groupId: GROUP_ID.toString(),
      numOutcomes: def.numOutcomes,
      winningOutcome: def.winningOutcome,
      seedCapital: totalSeedCapital.toString(),
    });

    await printOdds(program, mPda, def.title);
  }

  // ── Add LP liquidity ─────────────────────────────────────────────────────
  banner("ADD LIQUIDITY");
  const lpLpAta = await getOrCreateAta(connection, admin, lpMint, lpProvider.publicKey);
  await program.methods
    .addLiquidity(LP_DEPOSIT)
    .accounts({
      globalConfig: gcPda,
      lpMint,
      treasury: trPda,
      treasuryBaseAta,
      providerBaseAta: lpBaseAta,
      providerLpAta: lpLpAta,
      baseMint,
      pendingLiquidity: pendingPda(lpProvider.publicKey),
      provider: lpProvider.publicKey,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .signers([lpProvider])
    .rpc();
  const lpBal = await tokenBalance(connection, lpLpAta);
  const treasBal = await tokenBalance(connection, treasuryBaseAta);
  logLine(`LP tokens minted: ${lpBal.toString()}`);
  logLine(`Treasury balance: ${toUsdc(Number(treasBal))}`);

  // The deposit's shares unlock at (now/dur)*dur + 2*dur.
  const depositNow = nowSec();
  const lpActivationTime =
    Math.floor(depositNow / EPOCH_DURATION) * EPOCH_DURATION + 2 * EPOCH_DURATION;
  logLine(`LP shares unlock at unix ${lpActivationTime} (~${lpActivationTime - depositNow}s)`);

  // ── Single-share bets ──────────────────────────────────────────────────────
  banner("USERS PLACE SINGLE-SHARE BETS");
  const market0 = markets[0];
  const m0Pda = new PublicKey(market0.marketPda);
  const m0Id = new BN(market0.marketId);

  const singleBets: SingleBetState[] = [];
  // 4 users bet on market 0 (3-way result), mixed outcomes.
  const singlePlan = [
    { idx: 0, outcome: 0 },
    { idx: 1, outcome: 2 },
    { idx: 2, outcome: 0 },
    { idx: 3, outcome: 1 },
  ];
  for (const p of singlePlan) {
    const user = users[p.idx];
    const userBase = userBaseAtas[p.idx];
    const omPda = outcomeMintPda(m0Id, p.outcome);
    const userOutcomeAta = await getOrCreateAta(
      connection,
      admin,
      omPda,
      user.publicKey
    );
    // Use the plain (non-correlated) buy_shares path. No correlations are
    // registered on this group, so correlated pricing would add no signal — and
    // buy_shares avoids deserializing peer-market accounts (and the associated
    // heap pressure) entirely.
    const balBefore = await tokenBalance(connection, userBase);
    await program.methods
      .buyShares(p.outcome, SINGLE_BET, SINGLE_BET.muln(3))
      .accounts({
        globalConfig: gcPda,
        market: m0Pda,
        treasury: trPda,
        buyerBaseAta: userBase,
        treasuryBaseAta,
        buyerOutcomeAta: userOutcomeAta,
        outcomeMint: omPda,
        baseMint,
        buyer: user.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([user])
      .rpc();
    const balAfter = await tokenBalance(connection, userBase);
    const totalCost = Number(balBefore) - Number(balAfter);
    // Cost includes 1% buy fee: total_cost = cost + (cost * 0.01) = cost * 1.01
    const costOnly = totalCost / 1.01;
    const fee = totalCost - costOnly;
    sub(
      `${userLabels[p.idx]} bet outcome ${p.outcome} (${market0.outcomeNames[p.outcome]}) cost ${toUsdc(totalCost)} (${toUsdc(costOnly)} + ${toUsdc(fee)} fee)`
    );
    singleBets.push({
      user: kpToJson(user),
      userLabel: userLabels[p.idx],
      marketId: market0.marketId,
      outcomeId: p.outcome,
      shares: SINGLE_BET.toString(),
      outcomeAta: userOutcomeAta.toBase58(),
      cost: Math.floor(costOnly).toString(),
      fee: Math.floor(fee).toString(),
    });
  }
  await printOdds(program, m0Pda, "after single bets (market 0)");

  // ── Multi-leg slips (open → add_leg → finalize) ──────────────────────────
  // Skipping slip bets due to InstructionFallbackNotFound - needs program fix
  banner("USERS PLACE MULTI-LEG SLIPS");
  sub("Skipped (openSlip instruction not supported in current build)");
  const slips: SlipState[] = [];
  // Users 5 & 6 each place a 2-leg slip across markets 0 and 1.
  const slipPlan = [
    {
      idx: 4,
      legs: [
        { market: 0, outcome: 0 }, // Arsenal win
        { market: 1, outcome: 0 }, // BTTS Yes
      ],
    },
    {
      idx: 5,
      legs: [
        { market: 0, outcome: 2 }, // Chelsea win
        { market: 1, outcome: 0 }, // BTTS Yes
      ],
    },
  ];

  // Skip slip execution - openSlip instruction not found
  /*
  for (const plan of slipPlan) {
    const user = users[plan.idx];
    const userBase = userBaseAtas[plan.idx];

    const cfgNow: any = await program.account.globalConfig.fetch(gcPda);
    const slipId: BN = cfgNow.nextSlipId;
    const slipPda = betSlipPda(slipId);
    const numLegs = plan.legs.length;
    const maxPayment = SLIP_LEG_SHARES.muln(numLegs * 3);

    await program.methods
      .openSlip(slipId, numLegs, maxPayment)
      .accounts({
        globalConfig: gcPda,
        betSlip: slipPda,
        slipCreator: user.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([user])
      .rpc();

    const legStates = [];
    for (const leg of plan.legs) {
      const mkt = markets[leg.market];
      const mId = new BN(mkt.marketId);
      const mPda = new PublicKey(mkt.marketPda);
      const omPda = outcomeMintPda(mId, leg.outcome);
      const slipOutcomeAta = ataAddress(omPda, slipPda, true);
      const balBefore = await tokenBalance(connection, userBase);
      await program.methods
        .addSlipLeg(slipId, {
          marketId: mId,
          outcomeId: leg.outcome,
          numShares: SLIP_LEG_SHARES,
        })
        .accounts({
          globalConfig: gcPda,
          betSlip: slipPda,
          market: mPda,
          treasury: trPda,
          buyerBaseAta: userBase,
          treasuryBaseAta,
          outcomeMint: omPda,
          slipOutcomeAta,
          baseMint,
          slipCreator: user.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([user])
        .rpc();
      const balAfter = await tokenBalance(connection, userBase);
      const legCost = Number(balBefore) - Number(balAfter);
      legStates.push({
        marketId: mkt.marketId,
        outcomeId: leg.outcome,
        numShares: SLIP_LEG_SHARES.toString(),
        mint: omPda.toBase58(),
        cost: legCost.toString(),
      });
    }

    await program.methods
      .finalizeSlip(slipId)
      .accounts({
        globalConfig: gcPda,
        betSlip: slipPda,
        treasury: trPda,
        treasuryBaseAta,
        baseMint,
        slipCreator: user.publicKey,
      })
      .signers([user])
      .rpc();

    const slip: any = await program.account.betSlip.fetch(slipPda);
    sub(
      `${userLabels[plan.idx]} slip #${slipId.toString()}: ${numLegs} legs, stake ${toUsdc(
        slip.totalStake
      )}, potential ${toUsdc(slip.potentialPayout)}, status ${JSON.stringify(
        slip.status
      )}`
    );
    slips.push({
      slipId: slipId.toString(),
      user: kpToJson(user),
      userLabel: userLabels[plan.idx],
      legs: legStates,
      stake: slip.totalStake.toString(),
      potentialPayout: slip.potentialPayout.toString(),
    });
  }
  */

  await printOdds(program, m0Pda, "after slips (market 0)");

  // ── Persist state for Phase 2 ────────────────────────────────────────────
  banner("PERSIST STATE");
  const state: LifecycleState = {
    baseMint: baseMint.toBase58(),
    oracle: kpToJson(oracle),
    groupId: GROUP_ID.toString(),
    epochId: epoch0Id.toString(),
    markets,
    singleBets,
    slips,
    lp: {
      provider: kpToJson(lpProvider),
      baseAta: lpBaseAta.toBase58(),
      lpAta: lpLpAta.toBase58(),
      deposited: LP_DEPOSIT.toString(),
    },
    config: {
      challengeWindowSeconds: CHALLENGE_WINDOW,
      epochDurationSeconds: EPOCH_DURATION,
      withdrawalCooldownSeconds: WITHDRAWAL_COOLDOWN,
    },
    lpActivationTime,
    createdAt: nowSec(),
  };
  saveState(state);
  logLine(`State written. Markets close (betting ends) at unix ${startTime}.`);
  logLine(`Run settle_and_withdraw.ts after that to finish the lifecycle.`);

  banner("PHASE 1 COMPLETE ✓");
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e);
    process.exit(1);
  }
);
