/**
 * lifecycle/settle_and_withdraw.ts  — PHASE 2
 * ─────────────────────────────────────────────────────────────────────────────
 * Loads lifecycle/state.json (written by open_and_bet.ts) and drives the rest of
 * the protocol lifecycle on a live localnet:
 *   1. wait for betting to close (now >= market.start_time)
 *   2. settle each market: propose_result (oracle) → admin_override → finalize
 *   3. single-share winners claim_payout; slip holders claim_slip
 *   4. wait for the LP deposit's activation lock, then request_withdraw + process
 *
 * Run:
 *   npx ts-node lifecycle/settle_and_withdraw.ts
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
  getOrCreateAta,
  ataAddress,
  tokenBalance,
  waitUntilChainTime,
  chainTime,
  loadState,
  kpFromJson,
  globalConfigPda,
  treasuryPda,
  lpMintPda,
  marketPda,
  marketGroupPda,
  outcomeMintPda,
  epochPda,
  disputePda,
  betSlipPda,
  pendingPda,
  withdrawalPda,
  printOdds,
} from "./common";
import { ASSOCIATED_TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { SystemProgram, ComputeBudgetProgram } from "@solana/web3.js";

// Per-market pool tracking (Polymarket-style)
interface MarketPool {
  marketId: string;
  title: string;
  seedCapital: number;        // Initial seed
  betsReceived: number;       // Total bet volume (cost only, no fees)
  feesCollected: number;      // Total fees from this market
  payoutsGiven: number;       // Total payouts to winners
  winningLegsToLP: number;    // Value from winning legs of losing slips
  finalBacking: number;       // market.backing after settlement
  netProfitLoss: number;      // (seeds + bets + fees) - payouts
}

// Accounting tracker
interface Accounting {
  initial: {
    treasuryBalance: number;
    lpDeposit: number;
    marketSeeds: { [marketId: string]: number };
  };
  betsCollected: {
    singleBets: number;
    slipStakes: number;
    totalBuyFees: number;
    totalSlipMargins: number;
  };
  payouts: {
    singleWinners: number;
    seedWinners: number;
    seedFeeRewards: number;
    slipWinners: number;
    winningLegValueToLP: number; // New: winning legs from losing slips
  };
  final: {
    treasuryBalance: number;
    lpWithdrawn: number;
    marketBackings: { [marketId: string]: number };
    lockedPayouts: number;
  };
  marketPools: { [marketId: string]: MarketPool }; // Per-market breakdown
}

async function main() {
  const { connection, program, admin } = makeCtx();
  const state = loadState();

  const gcPda = globalConfigPda();
  const trPda = treasuryPda();
  const lpMint = lpMintPda();
  const baseMint = new PublicKey(state.baseMint);
  const oracle = kpFromJson(state.oracle);
  const epochId = new BN(state.epochId);
  const treasuryBaseAta = ataAddress(baseMint, trPda, true);

  // Initialize accounting tracker
  const accounting: Accounting = {
    initial: {
      treasuryBalance: 0,
      lpDeposit: Number(state.lp.deposited) / ONE_USDC,
      marketSeeds: {},
    },
    betsCollected: {
      singleBets: 0,
      slipStakes: 0,
      totalBuyFees: 0,
      totalSlipMargins: 0,
    },
    payouts: {
      singleWinners: 0,
      seedWinners: 0,
      seedFeeRewards: 0,
      slipWinners: 0,
      winningLegValueToLP: 0,
    },
    final: {
      treasuryBalance: 0,
      lpWithdrawn: 0,
      marketBackings: {},
      lockedPayouts: 0,
    },
    marketPools: {},
  };

  // Initialize market pools
  for (const mkt of state.markets) {
    const seedCapital = Number(mkt.seedCapital) / ONE_USDC;
    accounting.initial.marketSeeds[mkt.marketId] = seedCapital;
    accounting.marketPools[mkt.marketId] = {
      marketId: mkt.marketId,
      title: `Market ${mkt.marketId}`,
      seedCapital: seedCapital,
      betsReceived: 0,
      feesCollected: 0,
      payoutsGiven: 0,
      winningLegsToLP: 0,
      finalBacking: 0,
      netProfitLoss: 0,
    };
  }

  // Calculate revenue from state.json
  for (const bet of state.singleBets) {
    const cost = Number(bet.cost) / ONE_USDC;
    const fee = Number(bet.fee) / ONE_USDC;
    accounting.betsCollected.singleBets += cost;
    accounting.betsCollected.totalBuyFees += fee;
    
    // Track per-market
    const pool = accounting.marketPools[bet.marketId];
    pool.betsReceived += cost;
    pool.feesCollected += fee;
  }
  
  for (const slip of state.slips) {
    const stake = Number(slip.stake) / ONE_USDC;
    accounting.betsCollected.slipStakes += stake;

    for (const leg of slip.legs) {
      const pool = accounting.marketPools[leg.marketId];
      const legCost = Number(leg.cost ?? leg.numShares) / ONE_USDC;
      pool.betsReceived += legCost;
    }
  }

  // Get initial treasury balance
  const initialTreasury = await tokenBalance(connection, treasuryBaseAta);
  accounting.initial.treasuryBalance = Number(initialTreasury) / ONE_USDC;

  banner("PHASE 2 — SETTLE, CLAIM & WITHDRAW");
  logLine(`Markets:      ${state.markets.length}`);
  logLine(`Single bets:  ${state.singleBets.length}`);
  logLine(`Slips:        ${state.slips.length}`);

  // ── 1. Wait for betting to close on all markets ──────────────────────────
  banner("WAIT FOR BETTING TO CLOSE");
  const latestStart = Math.max(...state.markets.map((m) => m.startTime));
  await waitUntilChainTime(
    connection,
    latestStart,
    "betting close (now >= start_time)"
  );

  // ── 2. Settle each market ──────────────────────────────────────────────────
  banner("SETTLE MARKETS");
  for (const mkt of state.markets) {
    const mId = new BN(mkt.marketId);
    const mPda = new PublicKey(mkt.marketPda);
    const dPda = disputePda(mId);
    const win = mkt.winningOutcome;

    logLine(
      `Market ${mkt.marketId} (${mkt.outcomeNames[win]} wins) — proposing...`
    );

    // Oracle proposes the winning outcome (requires now >= start_time).
    await program.methods
      .proposeResult(mId, win)
      .accounts({
        globalConfig: gcPda,
        market: mPda,
        dispute: dPda,
        oracle: oracle.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .signers([oracle])
      .rpc();
    sub(`proposed outcome ${win}`);

    // Admin override sets the dispute Overridden so finalize runs immediately
    // (no need to wait out the challenge window).
    await program.methods
      .adminOverride(mId, win)
      .accounts({
        globalConfig: gcPda,
        market: mPda,
        dispute: dPda,
        admin: admin.publicKey,
      })
      .rpc();
    sub(`admin override → dispute Overridden`);

    // Finalize (permissionless). Overridden disputes finalize without waiting.
    await program.methods
      .finalizeResult(mId)
      .accounts({
        globalConfig: gcPda,
        market: mPda,
        dispute: dPda,
        epoch: epochPda(epochId),
        caller: admin.publicKey,
      })
      .rpc();

    const m: any = await program.account.market.fetch(mPda);
    sub(
      `finalized → status ${JSON.stringify(m.status)}, winning_outcome ${m.winningOutcome}`
    );
  }

  // ── 2b. Claim seed winners ────────────────────────────────────────────────
  banner("SEED CLAIMS");
  const adminBaseAta = ataAddress(baseMint, admin.publicKey);
  for (const mkt of state.markets) {
    const marketPda = new PublicKey(mkt.marketPda);
    const market: any = await program.account.market.fetch(marketPda);
    const winningMint = new PublicKey(market.outcomeMints[market.winningOutcome]);
    const adminWinningAta = await getOrCreateAta(
      connection,
      admin,
      winningMint,
      admin.publicKey
    );
    const balBefore = await tokenBalance(connection, adminBaseAta);
    await program.methods
      .claimPayout(new BN(mkt.marketId))
      .accounts({
        globalConfig: gcPda,
        market: marketPda,
        treasury: trPda,
        claimerOutcomeAta: adminWinningAta,
        claimerBaseAta: adminBaseAta,
        treasuryBaseAta,
        outcomeMint: winningMint,
        baseMint,
        claimer: admin.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .signers([admin])
      .rpc();
    const balAfter = await tokenBalance(connection, adminBaseAta);
    const payout = Number(balAfter) - Number(balBefore);
    const payoutUsdc = payout / ONE_USDC;
    accounting.payouts.seedWinners += payoutUsdc;
    accounting.marketPools[mkt.marketId].payoutsGiven += payoutUsdc;
    sub(
      `Admin seed claim market ${mkt.marketId}: claimed ${toUsdc(payout)} from winning outcome ${mkt.outcomeNames[market.winningOutcome]}`
    );
  }

  const mgPda = marketGroupPda(new BN(state.groupId));
  const marketGroup: any = await program.account.marketGroup.fetch(mgPda);

  // Claim the seed-fee rewards accumulated on losing seed positions.
  // These are real protocol liabilities, so they need to be realized before LP withdrawal.
  banner("SEED FEE REWARDS");
  for (let i = 0; i < marketGroup.numSeedPositions; i++) {
    const seed = marketGroup.seedPositions[i];
    const market = state.markets[seed?.marketIndex];
    if (!market) {
      sub(`Seed #${i}: skipped (no matching market)`);
      continue;
    }
    if (seed.rewardClaimed || seed.refunded) {
      sub(`Seed #${i}: skipped (already claimed/refunded)`);
      continue;
    }
    if (seed.outcomeId === market.winningOutcome) {
      sub(`Seed #${i}: skipped (winning outcome)`);
      continue;
    }

    const marketPda = new PublicKey(market.marketPda);
    const claimerBaseAta = adminBaseAta;
    const balBefore = await tokenBalance(connection, claimerBaseAta);
    try {
      await program.methods
        .claimSeedFeeReward(new BN(state.groupId), i)
        .accounts({
          globalConfig: gcPda,
          marketGroup: mgPda,
          market: marketPda,
          treasury: trPda,
          treasuryBaseAta,
          claimerBaseAta,
          baseMint,
          claimer: admin.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([admin])
        .rpc();
      const balAfter = await tokenBalance(connection, claimerBaseAta);
      const reward = Number(balAfter) - Number(balBefore);
      const rewardUsdc = reward / ONE_USDC;
      accounting.payouts.seedFeeRewards += rewardUsdc;
      accounting.marketPools[market.marketId].payoutsGiven += rewardUsdc;
      sub(
        `Seed #${i} (${market.marketId}, outcome ${seed.outcomeId}): claimed ${rewardUsdc.toFixed(4)} USDC`
      );
    } catch (e) {
      sub(
        `Seed #${i} (${market.marketId}, outcome ${seed.outcomeId}): skipped (${e.message?.includes('InvalidAmount') ? 'zero reward' : 'error'})`
      );
    }
  }

  const epochAfter: any = await program.account.epoch.fetch(epochPda(epochId));
  logLine(
    `Epoch ${state.epochId}: settled ${epochAfter.numSettledMarkets}/${epochAfter.numMarkets}, all_settled=${epochAfter.allMarketsSettled}, withdrawals_enabled=${epochAfter.withdrawalsEnabled}`
  );

  // ── 3a. Single-share winners claim payout ──────────────────────────────────
  banner("SINGLE-SHARE CLAIMS");
  for (const bet of state.singleBets) {
    const mkt = state.markets.find((m) => m.marketId === bet.marketId)!;
    const won = bet.outcomeId === mkt.winningOutcome;
    const user = kpFromJson(bet.user);
    const mId = new BN(bet.marketId);
    const mPda = new PublicKey(mkt.marketPda);
    const winningMint = new PublicKey(mkt.outcomeMints[mkt.winningOutcome]);

    if (!won) {
      sub(
        `${bet.userLabel}: outcome ${bet.outcomeId} (${mkt.outcomeNames[bet.outcomeId]}) LOST — position expires worthless`
      );
      continue;
    }

    const userBase = ataAddress(baseMint, user.publicKey);
    const userWinningAta = await getOrCreateAta(
      connection,
      admin,
      winningMint,
      user.publicKey
    );
    const balBefore = await tokenBalance(connection, userBase);
    await program.methods
      .claimPayout(mId)
      .accounts({
        globalConfig: gcPda,
        market: mPda,
        treasury: trPda,
        claimerOutcomeAta: userWinningAta,
        claimerBaseAta: userBase,
        treasuryBaseAta,
        outcomeMint: winningMint,
        baseMint,
        claimer: user.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      })
      .signers([user])
      .rpc();
    const balAfter = await tokenBalance(connection, userBase);
    const payout = Number(balAfter) - Number(balBefore);
    const payoutUsdc = payout / ONE_USDC;
    accounting.payouts.singleWinners += payoutUsdc;
    accounting.marketPools[bet.marketId].payoutsGiven += payoutUsdc;
    sub(
      `${bet.userLabel}: outcome ${bet.outcomeId} (${mkt.outcomeNames[bet.outcomeId]}) WON — claimed ${toUsdc(payout)}`
    );
  }

  // ── 3b. Slip holders claim ──────────────────────────────────────────────────
  banner("SLIP CLAIMS");
  for (const slip of state.slips) {
    const user = kpFromJson(slip.user);
    const slipId = new BN(slip.slipId);
    const slipPda = betSlipPda(slipId);

    // Determine if every leg won (all-or-nothing).
    const allWon = slip.legs.every((leg) => {
      const mkt = state.markets.find((m) => m.marketId === leg.marketId)!;
      return leg.outcomeId === mkt.winningOutcome;
    });

    // remaining_accounts: [market, outcome_mint, slip_outcome_ata] per leg.
    const remaining: anchor.web3.AccountMeta[] = [];
    for (const leg of slip.legs) {
      const mkt = state.markets.find((m) => m.marketId === leg.marketId)!;
      const mPda = new PublicKey(mkt.marketPda);
      const mint = new PublicKey(leg.mint);
      const slipOutcomeAta = ataAddress(mint, slipPda, true);
      remaining.push({ pubkey: mPda, isSigner: false, isWritable: true });
      remaining.push({ pubkey: mint, isSigner: false, isWritable: true });
      remaining.push({ pubkey: slipOutcomeAta, isSigner: false, isWritable: true });
    }

    const userBase = ataAddress(baseMint, user.publicKey);
    const balBefore = await tokenBalance(connection, userBase);
    await program.methods
      .claimSlip(slipId, 0)
      .accounts({
        globalConfig: gcPda,
        betSlip: slipPda,
        treasury: trPda,
        claimerBaseAta: userBase,
        treasuryBaseAta,
        baseMint,
        claimer: user.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      // claim_slip deserializes a Market per leg (heap-allocated strings) — give
      // the custom allocator a larger heap frame so multi-leg claims fit.
      .preInstructions([
        ComputeBudgetProgram.requestHeapFrame({ bytes: 256 * 1024 }),
        ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      ])
      .remainingAccounts(remaining)
      .signers([user])
      .rpc();
    const balAfter = await tokenBalance(connection, userBase);
    const delta = Number(balAfter) - Number(balBefore);
    if (allWon) {
      const payoutUsdc = delta / ONE_USDC;
      accounting.payouts.slipWinners += payoutUsdc;
      // Split payout across markets proportionally to their contribution
      const totalLegCost = slip.legs.reduce((sum, leg) => {
        return sum + Number(leg.cost ?? leg.numShares) / ONE_USDC;
      }, 0);
      for (const leg of slip.legs) {
        const legCost = Number(leg.cost ?? leg.numShares) / ONE_USDC;
        const legShare = totalLegCost > 0 ? legCost / totalLegCost : 0;
        accounting.marketPools[leg.marketId].payoutsGiven += payoutUsdc * legShare;
      }
    } else {
      // Calculate winning leg value that goes to LP
      for (const leg of slip.legs) {
        const mkt = state.markets.find((m) => m.marketId === leg.marketId)!;
        if (leg.outcomeId === mkt.winningOutcome) {
          const winningLegValue = Number(leg.numShares) / ONE_USDC; // 1:1 at settlement
          accounting.payouts.winningLegValueToLP += winningLegValue;
          accounting.marketPools[leg.marketId].winningLegsToLP += winningLegValue;
        }
      }
    }
    sub(
      `${slip.userLabel} slip #${slip.slipId}: ${allWon ? "ALL LEGS WON" : "LOST (no payout)"} — net ${toUsdc(delta)}`
    );
  }

  // ── 4. LP withdrawal ─────────────────────────────────────────────────────
  banner("LP WITHDRAWAL");
  const lp = kpFromJson(state.lp.provider);
  const lpBaseAta = new PublicKey(state.lp.baseAta);
  const lpLpAta = new PublicKey(state.lp.lpAta);
  const treasuryLpAta = await getOrCreateAta(connection, admin, lpMint, trPda, true);

  // The LP deposit's shares are locked until lpActivationTime.
  await waitUntilChainTime(
    connection,
    state.lpActivationTime,
    "LP shares unlock"
  );

  const lpShares = await tokenBalance(connection, lpLpAta);
  logLine(`LP shares to withdraw: ${lpShares.toString()}`);

  // request_withdraw validates the epoch account against global_config.current_epoch,
  // which advance_epoch (called inside add_liquidity) rolled forward to ~now/dur.
  // That epoch was never initialized; init it now (idempotent). An epoch with 0
  // markets is created with withdrawals_enabled = true, which is exactly what the
  // withdrawal gate requires.
  const cfgNow: any = await program.account.globalConfig.fetch(gcPda);
  const currentEpochId: BN = cfgNow.currentEpoch;
  const currentEpochPda = epochPda(currentEpochId);
  logLine(`current_epoch is ${currentEpochId.toString()} (markets settled in epoch ${state.epochId})`);
  await program.methods
    .initEpoch()
    .accounts({
      globalConfig: gcPda,
      epoch: currentEpochPda,
      authority: admin.publicKey,
      systemProgram: SystemProgram.programId,
    })
    .rpc();
  const ce: any = await program.account.epoch.fetch(currentEpochPda);
  logLine(
    `current_epoch ${currentEpochId.toString()}: withdrawals_enabled=${ce.withdrawalsEnabled}`
  );

  // request_withdraw
  await program.methods
    .requestWithdraw(new BN(lpShares.toString()))
    .accounts({
      globalConfig: gcPda,
      lpMint,
      treasury: trPda,
      treasuryBaseAta,
      treasuryLpAta,
      lpLpAta,
      pendingLiquidity: pendingPda(lp.publicKey),
      withdrawalRequest: withdrawalPda(lp.publicKey),
      baseMint,
      epoch: currentEpochPda,
      lp: lp.publicKey,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .signers([lp])
    .rpc();
  const req: any = await program.account.withdrawalRequest.fetch(
    withdrawalPda(lp.publicKey)
  );
  logLine(
    `Withdrawal requested: ${req.shares.toString()} shares, cooldown_end unix ${req.cooldownEnd}`
  );

  // Wait out the (possibly zero) cooldown.
  await waitUntilChainTime(
    connection,
    Number(req.cooldownEnd),
    "withdrawal cooldown"
  );

  const balBefore = await tokenBalance(connection, lpBaseAta);
  await program.methods
    .processWithdrawal()
    .accounts({
      globalConfig: gcPda,
      lpMint,
      treasury: trPda,
      treasuryBaseAta,
      treasuryLpAta,
      lpBaseAta,
      baseMint,
      withdrawalRequest: withdrawalPda(lp.publicKey),
      authority: lp.publicKey,
      tokenProgram: TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .signers([lp])
    .rpc();
  const balAfter = await tokenBalance(connection, lpBaseAta);
  const returned = Number(balAfter) - Number(balBefore);
  accounting.final.lpWithdrawn = returned / ONE_USDC;
  logLine(`LP withdrawal processed — returned ${toUsdc(returned)}`);
  logLine(`LP deposited ${toUsdc(Number(state.lp.deposited))}, got back ${toUsdc(returned)}`);

  // ── Final summary ─────────────────────────────────────────────────────────
  banner("FINAL SUMMARY");
  const cfg: any = await program.account.globalConfig.fetch(gcPda);
  const treas = await tokenBalance(connection, treasuryBaseAta);
  accounting.final.treasuryBalance = Number(treas) / ONE_USDC;
  accounting.final.lockedPayouts = cfg.lockedPayouts / ONE_USDC;
  
  for (const mkt of state.markets) {
    const m: any = await program.account.market.fetch(new PublicKey(mkt.marketPda));
    const finalBacking = m.backing.toNumber() / ONE_USDC;
    accounting.final.marketBackings[mkt.marketId] = finalBacking;
    
    // Update pool and calculate net P&L
    const pool = accounting.marketPools[mkt.marketId];
    pool.finalBacking = finalBacking;
    pool.netProfitLoss = pool.seedCapital + pool.betsReceived + pool.feesCollected - pool.payoutsGiven;
    
    logLine(
      `Market ${mkt.marketId}: status ${JSON.stringify(m.status)}, winner ${m.winningOutcome} (${mkt.outcomeNames[m.winningOutcome]}), backing ${toUsdc(m.backing)}`
    );
  }

  // ── Market Pool Breakdown (Polymarket-style) ──────────────────────────────
  banner("MARKET POOLS");
  
  logLine("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
  for (const [marketId, pool] of Object.entries(accounting.marketPools)) {
    const market = state.markets.find(m => m.marketId === marketId)!;
    logLine(`📊 MARKET ${marketId}: ${market.outcomeNames.join(' vs ')}`);
    logLine(`   Winner: ${market.outcomeNames[market.winningOutcome]}`);
    logLine(``);
    logLine(`   💰 Pool Flows:`);
    logLine(`      Seed capital:           ${pool.seedCapital.toFixed(2)} USDC`);
    logLine(`      Bets received:          ${pool.betsReceived.toFixed(2)} USDC`);
    logLine(`      Fees collected:         ${pool.feesCollected.toFixed(2)} USDC`);
    logLine(`      ─────────────────────────────────────`);
    logLine(`      Total inflow:           ${(pool.seedCapital + pool.betsReceived + pool.feesCollected).toFixed(2)} USDC`);
    logLine(``);
    logLine(`   💸 Payouts:`);
    logLine(`      Winners paid:           ${pool.payoutsGiven.toFixed(2)} USDC`);
    if (pool.winningLegsToLP > 0) {
      logLine(`      Liability released:     ${pool.winningLegsToLP.toFixed(2)} USDC`);
    }
    logLine(`      ─────────────────────────────────────`);
    logLine(`      Total outflow:          ${(pool.payoutsGiven).toFixed(2)} USDC`);
    logLine(``);
    logLine(`   🔒 Final State:`);
    logLine(`      Market backing:         ${pool.finalBacking.toFixed(2)} USDC`);
    const profitColor = pool.netProfitLoss >= 0 ? '✓' : '✗';
    logLine(`      Net P&L:                ${profitColor} ${pool.netProfitLoss >= 0 ? '+' : ''}${pool.netProfitLoss.toFixed(2)} USDC`);
    logLine(``);
    
    // Volume stats
    const totalVolume = pool.betsReceived;
    const returnRate = totalVolume > 0 ? (pool.payoutsGiven / totalVolume) * 100 : 0;
    logLine(`   📈 Statistics:`);
    logLine(`      Total volume:           ${totalVolume.toFixed(2)} USDC`);
    logLine(`      Payout ratio:           ${returnRate.toFixed(1)}% of volume`);
    logLine(`      House edge realized:    ${(100 - returnRate).toFixed(1)}%`);
    logLine(`━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━`);
    logLine(``);
  }

  // ── Comprehensive Accounting Report ──────────────────────────────────────
  banner("GLOBAL ACCOUNTING");
  
  logLine("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
  logLine("💰 INITIAL STATE");
  logLine(`  LP deposited:              ${accounting.initial.lpDeposit.toFixed(2)} USDC`);
  for (const [marketId, seed] of Object.entries(accounting.initial.marketSeeds)) {
    logLine(`  Market ${marketId} seed:         ${seed.toFixed(2)} USDC`);
  }
  const totalSeeds = Object.values(accounting.initial.marketSeeds).reduce((a, b) => a + b, 0);
  logLine(`  Total treasury start:      ${accounting.initial.treasuryBalance.toFixed(2)} USDC`);
  
  logLine("");
  logLine("📊 REVENUE COLLECTED");
  logLine(`  Single bet costs:          ${accounting.betsCollected.singleBets.toFixed(2)} USDC`);
  logLine(`  Single bet fees (1%):      ${accounting.betsCollected.totalBuyFees.toFixed(2)} USDC`);
  logLine(`  Slip stakes:               ${accounting.betsCollected.slipStakes.toFixed(2)} USDC`);
  logLine(`  Slip margins (not cash):   ${accounting.betsCollected.totalSlipMargins.toFixed(2)} USDC`);
  const totalCollected = accounting.betsCollected.singleBets + accounting.betsCollected.slipStakes;
  const totalFees = accounting.betsCollected.totalBuyFees + accounting.betsCollected.totalSlipMargins;
  logLine(`  Total collected:           ${totalCollected.toFixed(2)} USDC`);
  logLine(`  Total fees:                ${totalFees.toFixed(2)} USDC`);
  
  logLine("");
  logLine("💸 PAYOUTS MADE");
  logLine(`  Single bet winners:        ${accounting.payouts.singleWinners.toFixed(2)} USDC`);
  logLine(`  Seed winners:              ${accounting.payouts.seedWinners.toFixed(2)} USDC`);
  logLine(`  Seed fee rewards:          ${accounting.payouts.seedFeeRewards.toFixed(2)} USDC`);
  logLine(`  Slip winners:              ${accounting.payouts.slipWinners.toFixed(2)} USDC`);
  logLine(`  Liability released:        ${accounting.payouts.winningLegValueToLP.toFixed(2)} USDC`);
  const totalPaid =
    accounting.payouts.singleWinners +
    accounting.payouts.seedWinners +
    accounting.payouts.seedFeeRewards +
    accounting.payouts.slipWinners;
  logLine(`  Total paid to users:       ${totalPaid.toFixed(2)} USDC`);
  
  logLine("");
  logLine("🔒 FINAL STATE");
  logLine(`  Treasury balance:          ${accounting.final.treasuryBalance.toFixed(2)} USDC`);
  logLine(`  Global locked payouts:     ${accounting.final.lockedPayouts.toFixed(2)} USDC`);
  for (const [marketId, backing] of Object.entries(accounting.final.marketBackings)) {
    logLine(`  Market ${marketId} backing:       ${backing.toFixed(2)} USDC`);
  }
  const totalMarketBackings = Object.values(accounting.final.marketBackings).reduce((a, b) => a + b, 0);
  logLine(`  Total market backings:     ${totalMarketBackings.toFixed(2)} USDC`);
  
  logLine("");
  logLine("🎯 LP PROFIT/LOSS");
  logLine(`  LP withdrew:               ${accounting.final.lpWithdrawn.toFixed(2)} USDC`);
  logLine(`  LP deposited:              ${accounting.initial.lpDeposit.toFixed(2)} USDC`);
  const lpProfit = accounting.final.lpWithdrawn - accounting.initial.lpDeposit;
  const lpProfitPct = (lpProfit / accounting.initial.lpDeposit) * 100;
  logLine(`  LP profit:                 ${lpProfit >= 0 ? '+' : ''}${lpProfit.toFixed(2)} USDC (${lpProfitPct >= 0 ? '+' : ''}${lpProfitPct.toFixed(2)}%)`);
  
  logLine("");
  logLine("✅ RECONCILIATION");
  const expectedTreasury = accounting.initial.treasuryBalance + totalCollected - totalPaid - accounting.final.lpWithdrawn;
  const reconciled = Math.abs(expectedTreasury - accounting.final.treasuryBalance) < 0.01;
  logLine(`  Treasury start:            ${accounting.initial.treasuryBalance.toFixed(2)} USDC`);
  logLine(`  + Bets collected:          +${totalCollected.toFixed(2)} USDC`);
  logLine(`  - Payouts:                 -${totalPaid.toFixed(2)} USDC`);
  logLine(`  - LP withdrawn:            -${accounting.final.lpWithdrawn.toFixed(2)} USDC`);
  logLine(`  = Expected treasury:       ${expectedTreasury.toFixed(2)} USDC`);
  logLine(`  = Actual treasury:         ${accounting.final.treasuryBalance.toFixed(2)} USDC`);
  logLine(`  Difference:                ${(expectedTreasury - accounting.final.treasuryBalance).toFixed(2)} USDC`);
  logLine(`  Status:                    ${reconciled ? '✓ RECONCILED' : '✗ MISMATCH'}`);
  logLine(``);
  logLine(`  Note: Winning leg transfer (${accounting.payouts.winningLegValueToLP.toFixed(2)} USDC) reduces market.backing`);
  logLine(`        but is liability release, not a fresh treasury inflow.`);
  
  logLine("");
  logLine("📈 PROTOCOL REVENUE BREAKDOWN");
  const userNet = totalCollected - (accounting.payouts.singleWinners + accounting.payouts.slipWinners);
  const totalRevenue = totalFees + userNet - accounting.payouts.seedFeeRewards;
  logLine(`  Fees collected:            ${totalFees.toFixed(2)} USDC`);
  logLine(`  User net flow:             ${userNet.toFixed(2)} USDC`);
  logLine(`  Seed fee rewards:          ${accounting.payouts.seedFeeRewards.toFixed(2)} USDC`);
  logLine(`  Liability released:        ${accounting.payouts.winningLegValueToLP.toFixed(2)} USDC`);
  logLine(`  Total protocol revenue:    ${totalRevenue.toFixed(2)} USDC`);
  logLine(`  (Seed payouts excluded; matches LP profit: ${Math.abs(totalRevenue - lpProfit) < 0.01 ? '✓ YES' : '✗ NO'})`);
  
  logLine("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

  banner("PHASE 2 COMPLETE ✓");
}

main().then(
  () => process.exit(0),
  (e) => {
    console.error(e);
    process.exit(1);
  }
);
