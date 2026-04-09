/**
 * Quanductor E2E Test: Full Epoch Flow
 *
 * Runs the complete Quanductor flow against mainnet-forked data via Surfpool:
 *   0. Deploy program + time-travel to mainnet epoch
 *   1. Initialize ScoringState PDA
 *   2. Fetch all ValidatorHistory accounts from mainnet
 *   3. Crank scores (batch ~25 VH accounts per tx)
 *   4. Compute threshold (90th percentile)
 *   5. Create stake account + transfer staker authority to program PDA
 *   6. Delegate stake to a top-performing validator
 *
 * Prerequisites:
 *   - `quasar build` (program binary at target/deploy/quanductor.so)
 *   - `surfpool start` running in another terminal
 *
 * Usage:
 *   cd tests/surfpool && npm install && npx tsx e2e-full-flow.ts
 */

import {
  Connection,
  Keypair,
  PublicKey,
  TransactionInstruction,
  Transaction,
  SystemProgram,
  StakeProgram,
  Authorized,
  Lockup,
  LAMPORTS_PER_SOL,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// ============================================================================
// Constants
// ============================================================================

const PROGRAM_ID = new PublicKey(
  "4qoALqJXrrjcqTmetedH55rvHTeF4XPfFVo8GaztD6KR",
);
const VH_PROGRAM_ID = new PublicKey(
  "HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa",
);
const USER_WALLET = new PublicKey(
  "GKPoVMxhDTi3SGxK8wRgguwfXjV8EmNkpqCfeQWyhniT",
);
const NATIVE_STAKE_PROGRAM = new PublicKey(
  "Stake11111111111111111111111111111111111111",
);

const CLOCK_SYSVAR = new PublicKey(
  "SysvarC1ock11111111111111111111111111111111",
);
const STAKE_HISTORY_SYSVAR = new PublicKey(
  "SysvarStakeHistory1111111111111111111111111",
);
const STAKE_CONFIG = new PublicKey(
  "StakeConfig11111111111111111111111111111111",
);

// PDAs
const [scoringStatePda] = PublicKey.findProgramAddressSync(
  [Buffer.from("scoring_state")],
  PROGRAM_ID,
);
const [stakeAuthorityPda] = PublicKey.findProgramAddressSync(
  [Buffer.from("stake_authority")],
  PROGRAM_ID,
);

// ValidatorHistory layout
const VH_DISCRIMINATOR = Buffer.from([205, 25, 8, 221, 253, 131, 2, 146]);
const VH_OFFSET_VOTE_ACCOUNT = 12;
const VH_OFFSET_CIRC_BUF_IDX = 304;
const VH_OFFSET_CIRC_BUF_IS_EMPTY = 312;
const VH_OFFSET_CIRC_BUF_ARR = 320;
const ENTRY_SIZE = 128;
const MAX_ENTRIES = 512;
const ENTRY_OFFSET_EPOCH = 8;
const ENTRY_OFFSET_EPOCH_CREDITS = 12;
const ENTRY_OFFSET_COMMISSION = 16;
const EPOCH_UNSET = 0xffff;
const COMMISSION_UNSET = 0xff;
const EPOCHS_LOOKBACK = 5;
const VH_MIN_DATA_LEN = 65_856;
const MIN_VALIDATORS = 100;

// ScoringState layout offsets
const SS_OFFSET_DISC = 0;
const SS_OFFSET_PHASE = 1;
const SS_OFFSET_EPOCH = 2;
const SS_OFFSET_THRESHOLD = 10;
const SS_OFFSET_TOTAL_SCORED = 18;

// Config
const SURFPOOL_URL = "http://127.0.0.1:8899";
const MAINNET_URL =
  "https://mainnet.helius-rpc.com/?api-key=1e722833-0c3e-47ac-9974-13de5c01d1ee";
const BATCH_SIZE = 25;
const STAKE_AMOUNT = 1 * LAMPORTS_PER_SOL;

// ============================================================================
// Surfpool RPC helper
// ============================================================================

async function surfnetCall(method: string, params: unknown[]): Promise<any> {
  const response = await fetch(SURFPOOL_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const json: any = await response.json();
  if (json.error) {
    throw new Error(`${method}: ${JSON.stringify(json.error)}`);
  }
  return json.result;
}

// ============================================================================
// Client-side VH score computation (mirrors on-chain logic)
// ============================================================================

function computeScoreFromVh(data: Buffer, currentEpoch: number): bigint {
  if (data[VH_OFFSET_CIRC_BUF_IS_EMPTY] !== 0) return 0n;

  const circIdx = Number(data.readBigUInt64LE(VH_OFFSET_CIRC_BUF_IDX));
  const minEpoch = currentEpoch - EPOCHS_LOOKBACK + 1;

  let totalReward = 0n;
  let validEpochs = 0;

  for (let i = 0; i < MAX_ENTRIES; i++) {
    const idx = circIdx >= i ? circIdx - i : MAX_ENTRIES + circIdx - i;
    const base = VH_OFFSET_CIRC_BUF_ARR + idx * ENTRY_SIZE;

    const epoch = data.readUInt16LE(base + ENTRY_OFFSET_EPOCH);
    if (epoch === EPOCH_UNSET) continue;
    if (epoch < minEpoch) break;

    if (epoch <= currentEpoch) {
      const commission = data[base + ENTRY_OFFSET_COMMISSION];
      if (commission !== COMMISSION_UNSET) {
        const credits = data.readUInt32LE(base + ENTRY_OFFSET_EPOCH_CREDITS);
        totalReward += (BigInt(credits) * BigInt(100 - commission)) / 100n;
        validEpochs++;
      }
      if (validEpochs >= EPOCHS_LOOKBACK) break;
    }
  }

  return validEpochs === 0 ? 0n : totalReward / BigInt(validEpochs);
}

function readVhVoteAccount(data: Buffer): PublicKey {
  return new PublicKey(
    data.subarray(VH_OFFSET_VOTE_ACCOUNT, VH_OFFSET_VOTE_ACCOUNT + 32),
  );
}

// ============================================================================
// Read ScoringState account
// ============================================================================

interface ScoringStateInfo {
  phase: number;
  epoch: bigint;
  threshold: bigint;
  totalScored: number;
}

async function readScoringState(
  connection: Connection,
): Promise<ScoringStateInfo> {
  // Use a fresh connection to avoid any caching
  const fresh = new Connection(SURFPOOL_URL, {
    commitment: "confirmed",
    disableRetryOnRateLimit: true,
  });
  const info = await fresh.getAccountInfo(scoringStatePda, "processed");
  if (!info) {
    // Retry once with a small delay
    await new Promise((r) => setTimeout(r, 1000));
    const retry = await fresh.getAccountInfo(scoringStatePda, "processed");
    if (!retry) throw new Error("ScoringState account not found");
    const d = Buffer.from(retry.data);
    return {
      phase: d[SS_OFFSET_PHASE],
      epoch: d.readBigUInt64LE(SS_OFFSET_EPOCH),
      threshold: d.readBigUInt64LE(SS_OFFSET_THRESHOLD),
      totalScored: d.readUInt16LE(SS_OFFSET_TOTAL_SCORED),
    };
  }
  const d = Buffer.from(info.data);
  return {
    phase: d[SS_OFFSET_PHASE],
    epoch: d.readBigUInt64LE(SS_OFFSET_EPOCH),
    threshold: d.readBigUInt64LE(SS_OFFSET_THRESHOLD),
    totalScored: d.readUInt16LE(SS_OFFSET_TOTAL_SCORED),
  };
}

// ============================================================================
// Step 0: Deploy program + time-travel
// ============================================================================

async function deployProgram(connection: Connection): Promise<void> {
  const soPath = path.resolve(__dirname, "../../target/deploy/quanductor.so");
  const keypairPath = path.resolve(
    __dirname,
    "../../target/deploy/quanductor-keypair.json",
  );
  if (!fs.existsSync(soPath)) {
    throw new Error(
      `Program binary not found at ${soPath}. Run 'quasar build' first.`,
    );
  }

  const programBytes = fs.readFileSync(soPath);
  console.log(
    `  Deploying ${(programBytes.length / 1024).toFixed(1)} KB program via solana CLI...`,
  );

  // Create a temp deployer keypair and fund it
  const deployer = Keypair.generate();
  const deployerPath = "/tmp/surfpool-deployer.json";
  fs.writeFileSync(deployerPath, JSON.stringify(Array.from(deployer.secretKey)));

  await surfnetCall("surfnet_setAccount", [
    deployer.publicKey.toBase58(),
    {
      lamports: 100 * LAMPORTS_PER_SOL,
      owner: SystemProgram.programId.toBase58(),
    },
  ]);

  // Deploy via solana CLI
  const { execSync } = await import("child_process");
  try {
    const output = execSync(
      `solana program deploy ` +
        `--url ${SURFPOOL_URL} ` +
        `--keypair ${deployerPath} ` +
        `--program-id ${keypairPath} ` +
        `${soPath}`,
      { encoding: "utf8", timeout: 60000 },
    );
    console.log(`  ${output.trim()}`);
  } catch (err: any) {
    throw new Error(`Program deploy failed: ${err.stderr || err.message}`);
  }

  // Verify
  const info = await connection.getAccountInfo(PROGRAM_ID);
  if (!info || !info.executable) {
    throw new Error("Program deployment verification failed");
  }
  console.log(`  Program deployed at ${PROGRAM_ID.toBase58()}`);
}

async function timeTravelToMainnetEpoch(mainnet: Connection): Promise<number> {
  const epochInfo = await mainnet.getEpochInfo();
  console.log(
    `  Mainnet epoch: ${epochInfo.epoch}, slot: ${epochInfo.absoluteSlot}`,
  );

  // Time travel Surfpool to match mainnet epoch
  try {
    await surfnetCall("surfnet_timeTravel", [{ epoch: epochInfo.epoch }]);
  } catch {
    // Fallback format
    try {
      await surfnetCall("surfnet_timeTravel", [
        { absoluteEpoch: epochInfo.epoch },
      ]);
    } catch (e2: any) {
      console.warn(`  Warning: time travel failed (${e2.message})`);
      console.warn("  Continuing with Surfpool's current epoch...");
    }
  }

  // Verify clock
  try {
    const clock = await surfnetCall("surfnet_getClock", []);
    console.log(`  Surfpool epoch: ${clock.epoch}`);
    return clock.epoch;
  } catch {
    // Fallback: read epoch from slot info
    const slotInfo = await new Connection(SURFPOOL_URL).getEpochInfo();
    console.log(`  Surfpool epoch: ${slotInfo.epoch}`);
    return slotInfo.epoch;
  }
}

async function airdropSol(
  connection: Connection,
  pubkey: PublicKey,
  sol: number,
): Promise<void> {
  try {
    const sig = await connection.requestAirdrop(pubkey, sol * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig, "confirmed");
  } catch {
    // Fallback: use surfnet_setAccount cheatcode
    await surfnetCall("surfnet_setAccount", [
      {
        pubkey: pubkey.toBase58(),
        update: {
          lamports: sol * LAMPORTS_PER_SOL,
          owner: SystemProgram.programId.toBase58(),
        },
      },
    ]);
  }
}

// ============================================================================
// Step 1: Initialize ScoringState PDA
// ============================================================================

async function initializeScoringState(
  connection: Connection,
  payer: Keypair,
): Promise<void> {
  // Check if already initialized
  const existing = await connection.getAccountInfo(scoringStatePda);
  if (existing && existing.data.length > 0) {
    console.log("  ScoringState already exists, skipping init");
    return;
  }

  const ix = new TransactionInstruction({
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: scoringStatePda, isSigner: false, isWritable: true },
      {
        pubkey: SystemProgram.programId,
        isSigner: false,
        isWritable: false,
      },
    ],
    programId: PROGRAM_ID,
    data: Buffer.from([0]),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(connection, tx, [payer], {
    commitment: "confirmed",
  });
  console.log(`  ScoringState PDA: ${scoringStatePda.toBase58()}`);
  console.log(`  Stake authority PDA: ${stakeAuthorityPda.toBase58()}`);
  console.log(`  tx: ${sig}`);

  // Check transaction logs
  const txInfo = await connection.getTransaction(sig, {
    commitment: "confirmed",
    maxSupportedTransactionVersion: 0,
  });
  if (txInfo?.meta?.logMessages) {
    console.log("  Logs:");
    for (const log of txInfo.meta.logMessages) {
      console.log(`    ${log}`);
    }
    if (txInfo.meta.err) {
      console.log(`  TX ERROR: ${JSON.stringify(txInfo.meta.err)}`);
    }
  }

  // Verify the account exists right after init
  const verify = await connection.getAccountInfo(scoringStatePda);
  console.log(`  Account after init: ${verify ? `exists, ${verify.data.length} bytes` : "NULL"}`);
  if (!verify) {
    // Try with raw RPC call
    const rawResult = await surfnetCall("getAccountInfo", [
      scoringStatePda.toBase58(),
      { encoding: "base64", commitment: "processed" },
    ]);
    console.log(`  Raw RPC result: ${JSON.stringify(rawResult?.value ? "exists" : "null")}`);
    throw new Error("ScoringState not found after init");
  }
}

// ============================================================================
// Step 2: Fetch VH addresses from mainnet
// ============================================================================

async function fetchVhAddresses(mainnet: Connection): Promise<PublicKey[]> {
  console.log("  Querying Helius for ValidatorHistory accounts...");

  // Use discriminator filter + dataSize to get all VH accounts
  const VH_DISC_BASE58 = "bJhvHLshp1w"; // base58 of [205,25,8,221,253,131,2,146]
  const accounts = await mainnet.getProgramAccounts(VH_PROGRAM_ID, {
    filters: [{ memcmp: { offset: 0, bytes: VH_DISC_BASE58 } }],
    dataSlice: { offset: 0, length: 1 }, // just need pubkeys
  });

  console.log(`  Found ${accounts.length} ValidatorHistory accounts`);

  if (accounts.length < MIN_VALIDATORS) {
    throw new Error(
      `Insufficient validators: ${accounts.length} < ${MIN_VALIDATORS}`,
    );
  }

  return accounts.map((a) => a.pubkey);
}

// ============================================================================
// Step 3: Crank scores
// ============================================================================

async function crankAllScores(
  connection: Connection,
  payer: Keypair,
  vhAddresses: PublicKey[],
): Promise<void> {
  const batches: PublicKey[][] = [];
  for (let i = 0; i < vhAddresses.length; i += BATCH_SIZE) {
    batches.push(vhAddresses.slice(i, i + BATCH_SIZE));
  }

  console.log(
    `  ${batches.length} batches of ${BATCH_SIZE} validators each`,
  );

  let ok = 0;
  let fail = 0;

  for (let i = 0; i < batches.length; i++) {
    const batch = batches[i];

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: false },
        { pubkey: scoringStatePda, isSigner: false, isWritable: true },
        ...batch.map((vh) => ({
          pubkey: vh,
          isSigner: false,
          isWritable: false,
        })),
      ],
      programId: PROGRAM_ID,
      data: Buffer.from([1]),
    });

    try {
      const tx = new Transaction().add(ix);
      await sendAndConfirmTransaction(connection, tx, [payer], {
        skipPreflight: true,
      });
      ok++;
    } catch (err: any) {
      fail++;
      if (fail <= 3) {
        const msg = err?.logs
          ? err.logs.join("\n")
          : err.message?.slice(0, 200);
        console.error(`  Batch ${i + 1} error: ${msg}`);
      }
    }

    if ((i + 1) % 10 === 0 || i === batches.length - 1) {
      process.stdout.write(
        `\r  Progress: ${i + 1}/${batches.length} (${ok} ok, ${fail} failed)`,
      );
    }
  }
  console.log(); // newline after progress

  const state = await readScoringState(connection);
  console.log(`  Phase: ${state.phase} (expected 1=CRANKING)`);
  console.log(`  Total scored: ${state.totalScored}`);
  console.log(`  Epoch: ${state.epoch}`);

  if (state.totalScored < MIN_VALIDATORS) {
    throw new Error(
      `Only ${state.totalScored} validators scored, need ${MIN_VALIDATORS}. ` +
        `${fail} batches failed. Check program logs.`,
    );
  }
}

// ============================================================================
// Step 4: Compute threshold
// ============================================================================

async function computeThreshold(
  connection: Connection,
  payer: Keypair,
): Promise<bigint> {
  const ix = new TransactionInstruction({
    keys: [{ pubkey: scoringStatePda, isSigner: false, isWritable: true }],
    programId: PROGRAM_ID,
    data: Buffer.from([2]),
  });

  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(connection, tx, [payer]);

  const state = await readScoringState(connection);
  console.log(`  Phase: ${state.phase} (expected 2=THRESHOLD_COMPUTED)`);
  console.log(`  Threshold: ${state.threshold}`);
  console.log(`  Total scored: ${state.totalScored}`);
  console.log(`  tx: ${sig}`);

  if (state.phase !== 2) {
    throw new Error(
      `Expected phase 2 (THRESHOLD_COMPUTED), got ${state.phase}`,
    );
  }

  return state.threshold;
}

// ============================================================================
// Step 5: Create stake account + transfer staker authority
// ============================================================================

async function createStakeAccount(
  connection: Connection,
  payer: Keypair,
): Promise<Keypair> {
  const stakeKeypair = Keypair.generate();

  // Create + init: staker=payer (we sign), withdrawer=user wallet
  const createTx = StakeProgram.createAccount({
    fromPubkey: payer.publicKey,
    stakePubkey: stakeKeypair.publicKey,
    authorized: new Authorized(payer.publicKey, USER_WALLET),
    lamports: STAKE_AMOUNT,
    lockup: new Lockup(0, 0, PublicKey.default),
  });

  const sig1 = await sendAndConfirmTransaction(connection, createTx, [
    payer,
    stakeKeypair,
  ]);
  console.log(`  Stake account: ${stakeKeypair.publicKey.toBase58()}`);
  console.log(`  Amount: ${STAKE_AMOUNT / LAMPORTS_PER_SOL} SOL`);
  console.log(`  Withdrawer: ${USER_WALLET.toBase58()}`);
  console.log(`  tx: ${sig1}`);

  // Transfer staker authority to program's stake_authority PDA
  const authTx = StakeProgram.authorize({
    stakePubkey: stakeKeypair.publicKey,
    authorizedPubkey: payer.publicKey,
    newAuthorizedPubkey: stakeAuthorityPda,
    stakeAuthorizationType: { index: 0 }, // Staker
  });

  const sig2 = await sendAndConfirmTransaction(connection, authTx, [payer]);
  console.log(`  Staker authority -> ${stakeAuthorityPda.toBase58()}`);
  console.log(`  tx: ${sig2}`);

  return stakeKeypair;
}

// ============================================================================
// Step 6: Find a top validator + delegate stake
// ============================================================================

async function delegateToTopValidator(
  surfpool: Connection,
  mainnet: Connection,
  payer: Keypair,
  stakeKeypair: Keypair,
  threshold: bigint,
  currentEpoch: number,
): Promise<void> {
  console.log("  Searching for validator above threshold...");

  // Fetch VH accounts from Helius with discriminator filter
  const VH_DISC_BASE58 = "bJhvHLshp1w";
  const vhAccounts = await mainnet.getProgramAccounts(VH_PROGRAM_ID, {
    filters: [{ memcmp: { offset: 0, bytes: VH_DISC_BASE58 } }],
    dataSlice: { offset: 0, length: 1 },
  });

  // Shuffle to avoid always picking the same validator
  const candidates = vhAccounts
    .map((a) => a.pubkey)
    .sort(() => Math.random() - 0.5)
    .slice(0, 100);

  for (const vhPubkey of candidates) {
    // Fetch VH data from Surfpool (lazy-forks from mainnet)
    const vhInfo = await surfpool.getAccountInfo(vhPubkey);
    if (!vhInfo || vhInfo.data.length < VH_MIN_DATA_LEN) continue;

    const vhData = Buffer.from(vhInfo.data);
    if (!vhData.subarray(0, 8).equals(VH_DISCRIMINATOR)) continue;

    const score = computeScoreFromVh(vhData, currentEpoch);
    if (score < threshold) continue;

    const voteAccount = readVhVoteAccount(vhData);
    console.log(`  Candidate found:`);
    console.log(`    VH:    ${vhPubkey.toBase58()}`);
    console.log(`    Vote:  ${voteAccount.toBase58()}`);
    console.log(`    Score: ${score} (threshold: ${threshold})`);

    const ix = new TransactionInstruction({
      keys: [
        { pubkey: scoringStatePda, isSigner: false, isWritable: false },
        {
          pubkey: stakeKeypair.publicKey,
          isSigner: false,
          isWritable: true,
        },
        { pubkey: vhPubkey, isSigner: false, isWritable: false },
        { pubkey: voteAccount, isSigner: false, isWritable: false },
        { pubkey: CLOCK_SYSVAR, isSigner: false, isWritable: false },
        {
          pubkey: STAKE_HISTORY_SYSVAR,
          isSigner: false,
          isWritable: false,
        },
        { pubkey: STAKE_CONFIG, isSigner: false, isWritable: false },
        { pubkey: stakeAuthorityPda, isSigner: false, isWritable: false },
        {
          pubkey: NATIVE_STAKE_PROGRAM,
          isSigner: false,
          isWritable: false,
        },
      ],
      programId: PROGRAM_ID,
      data: Buffer.from([3]),
    });

    try {
      const tx = new Transaction().add(ix);
      const sig = await sendAndConfirmTransaction(surfpool, tx, [payer]);
      console.log(`  Delegation succeeded!`);
      console.log(`  tx: ${sig}`);

      // Verify stake account state
      const stakeInfo = await surfpool.getAccountInfo(
        stakeKeypair.publicKey,
      );
      if (stakeInfo) {
        const stakeState = Buffer.from(stakeInfo.data).readUInt32LE(0);
        const voterPk = new PublicKey(
          Buffer.from(stakeInfo.data).subarray(124, 156),
        );
        console.log(`  Stake state: ${stakeState} (2=delegated)`);
        console.log(`  Delegated to: ${voterPk.toBase58()}`);
      }
      return;
    } catch (err: any) {
      const msg = err?.logs
        ? err.logs.slice(-3).join(" | ")
        : err.message?.slice(0, 200);
      console.log(`  Delegation attempt failed: ${msg}`);
      console.log("  Trying next validator...");
    }
  }

  throw new Error(
    "Could not delegate to any validator. All candidates failed.",
  );
}

// ============================================================================
// Main
// ============================================================================

async function main() {
  console.log("========================================================");
  console.log("  Quanductor E2E Test: Full Epoch Flow");
  console.log("  Against Mainnet Data via Surfpool");
  console.log("========================================================\n");

  // Verify Surfpool is running
  const surfpool = new Connection(SURFPOOL_URL, "confirmed");
  try {
    await surfpool.getVersion();
  } catch {
    throw new Error(
      `Surfpool not reachable at ${SURFPOOL_URL}. Start it with: surfpool start`,
    );
  }

  const mainnet = new Connection(MAINNET_URL, "confirmed");
  const payer = Keypair.generate();

  // --- Step 0: Setup ---
  console.log("[0] Setup\n");

  const currentEpoch = await timeTravelToMainnetEpoch(mainnet);
  await deployProgram(surfpool);
  await airdropSol(surfpool, payer.publicKey, 100);
  console.log(`  Payer: ${payer.publicKey.toBase58()} (100 SOL)\n`);

  // --- Step 1: Initialize ---
  console.log("[1] Initialize ScoringState\n");
  await initializeScoringState(surfpool, payer);
  console.log();

  // --- Step 2: Fetch VH addresses ---
  console.log("[2] Fetch ValidatorHistory Accounts\n");
  const vhAddresses = await fetchVhAddresses(mainnet);
  console.log(`  Using ${vhAddresses.length} VH accounts for cranking`);
  console.log();

  // --- Step 3: Crank scores ---
  console.log("[3] Crank Scores\n");
  await crankAllScores(surfpool, payer, vhAddresses);
  console.log();

  // --- Step 4: Compute threshold ---
  console.log("[4] Compute Threshold\n");
  const threshold = await computeThreshold(surfpool, payer);
  console.log();

  // --- Step 5: Create stake account ---
  console.log("[5] Create Stake Account\n");
  const stakeKeypair = await createStakeAccount(surfpool, payer);
  console.log();

  // --- Step 6: Delegate ---
  console.log("[6] Delegate Stake\n");
  await delegateToTopValidator(
    surfpool,
    mainnet,
    payer,
    stakeKeypair,
    threshold,
    currentEpoch,
  );

  // --- Summary ---
  console.log("\n========================================================");
  console.log("  FULL FLOW COMPLETED SUCCESSFULLY");
  console.log("========================================================");
  console.log(`  Program ID:       ${PROGRAM_ID.toBase58()}`);
  console.log(`  ScoringState:     ${scoringStatePda.toBase58()}`);
  console.log(`  Stake Authority:  ${stakeAuthorityPda.toBase58()}`);
  console.log(`  Stake Account:    ${stakeKeypair.publicKey.toBase58()}`);
  console.log(`  User Wallet:      ${USER_WALLET.toBase58()}`);
  console.log(`  Threshold:        ${threshold}`);
  console.log(`  Epoch:            ${currentEpoch}`);
}

main().catch((err) => {
  console.error("\nE2E test failed:", err);
  process.exit(1);
});
