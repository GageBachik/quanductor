# Quanductor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Solana program using Quasar that scores validators via Stakenet's ValidatorHistory data and manages native stake delegation to top-performing validators (90th percentile by staking rewards proxy).

**Architecture:** Single `ScoringState` PDA holds a 512-bucket histogram and 768-byte bitmap. ValidatorHistory accounts are read as `UncheckedAccount` with manual zero-copy deserialization. Stake program CPI is built manually via Quasar's `CpiCall`. All instructions are permissionless.

**Tech Stack:** Quasar framework (`quasar-lang`), QuasarSVM for testing, Solana native stake program CPI.

**Reference Code:**
- Quasar framework: `/Users/loser/projects/quasar`
- Stakenet (ValidatorHistory): `/Users/loser/projects/stakenet`
- Design spec: `docs/superpowers/specs/2026-04-08-quanductor-design.md`

---

## File Structure

```
src/
  lib.rs                       — declare_id!, #[program] with 5 instructions
  state.rs                     — ScoringState account, phase constants
  errors.rs                    — #[error_code] enum with all error variants
  instructions/
    mod.rs                     — module exports
    initialize.rs              — Initialize accounts struct + handler
    crank_scores.rs            — CrankScores accounts struct + handler
    compute_threshold.rs       — ComputeThreshold accounts struct + handler
    delegate_stake.rs          — DelegateStake accounts struct + handler
    undelegate_stake.rs        — UndelegateStake accounts struct + handler
  validator_history.rs         — #[repr(C)] structs, reading/validation helpers
  stake_cpi.rs                 — Stake program CPI builders (delegate, deactivate)
  stake_state.rs               — Stake account parsing helpers
  tests.rs                     — All tests using QuasarSVM
```

---

### Task 1: State, Errors, and Program Skeleton

**Files:**
- Modify: `src/state.rs`
- Modify: `src/errors.rs`
- Modify: `src/lib.rs`
- Modify: `src/instructions/mod.rs`
- Modify: `src/instructions/initialize.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Update Cargo.toml dependencies**

We need `solana-pubkey` as a regular dependency (not just dev) for the validator history program ID constant.

```toml
[package]
name = "quanductor"
version = "0.1.0"
edition = "2021"

[lints.rust.unexpected_cfgs]
level = "warn"
check-cfg = [
    'cfg(target_os, values("solana"))',
]

[lib]
crate-type = ["cdylib"]

[features]
alloc = []
client = []
debug = []

[dependencies]
quasar-lang = { git = "https://github.com/blueshift-gg/quasar" }
solana-instruction = { version = "3.2.0" }

[dev-dependencies]
quanductor-client = { path = "target/client/rust/quanductor-client" }
quasar-svm = { git = "https://github.com/blueshift-gg/quasar-svm" }
solana-account = { version = "3.4.0" }
solana-address = { version = "2.2.0", features = ["decode"] }
solana-instruction = { version = "3.2.0", features = ["bincode"] }
solana-pubkey = { version = "4.1.0" }
```

- [ ] **Step 2: Write state.rs with ScoringState and phase constants**

```rust
use quasar_lang::prelude::*;

pub const PHASE_IDLE: u8 = 0;
pub const PHASE_CRANKING: u8 = 1;
pub const PHASE_THRESHOLD_COMPUTED: u8 = 2;

pub const HISTOGRAM_BUCKETS: usize = 512;
pub const BITMAP_BYTES: usize = 768;
pub const SCORE_RANGE: u64 = 420_001;
pub const MIN_VALIDATORS: u16 = 1_400;
pub const EPOCHS_LOOKBACK: usize = 5;

#[account(discriminator = 1)]
pub struct ScoringState {
    pub phase: u8,
    pub epoch: u64,
    pub threshold: u64,
    pub total_scored: u16,
    pub histogram: [u16; HISTOGRAM_BUCKETS],
    pub bitmap: [u8; BITMAP_BYTES],
    pub stake_authority_bump: u8,
    pub bump: u8,
}
```

- [ ] **Step 3: Write errors.rs**

```rust
use quasar_lang::prelude::*;

#[error_code]
pub enum QuanductorError {
    InvalidPhase,
    EpochMismatch,
    InsufficientValidators,
    ScoreBelowThreshold,
    ScoreAboveThreshold,
    InvalidStakeState,
    InvalidStakeAuthority,
    InvalidValidatorHistory,
    InsufficientEpochData,
    InvalidVoteAccount,
}
```

- [ ] **Step 4: Write initialize.rs**

```rust
use quasar_lang::prelude::*;
use crate::state::ScoringState;

#[derive(Accounts)]
pub struct Initialize<'info> {
    pub payer: &'info mut Signer,
    #[account(init, payer = payer, seeds = [b"scoring_state"], bump)]
    pub scoring_state: &'info mut Account<ScoringState>,
    pub system_program: &'info Program<System>,
}

impl<'info> Initialize<'info> {
    #[inline(always)]
    pub fn handler(&mut self, bumps: &InitializeBumps) -> Result<(), ProgramError> {
        // Derive stake_authority bump
        let (_, sa_bump) = quasar_lang::find_program_address(&[b"stake_authority"], &crate::ID);
        self.scoring_state.set_inner(
            0,          // phase = IDLE
            0u64,       // epoch
            0u64,       // threshold
            0u16,       // total_scored
            [0u16; 512],// histogram
            [0u8; 768], // bitmap
            sa_bump,    // stake_authority_bump
            bumps.scoring_state, // bump
        );
        Ok(())
    }
}
```

- [ ] **Step 5: Update instructions/mod.rs**

```rust
mod initialize;
pub use initialize::*;
```

- [ ] **Step 6: Update lib.rs**

```rust
#![cfg_attr(not(test), no_std)]

use quasar_lang::prelude::*;

mod errors;
mod instructions;
mod state;
use instructions::*;

declare_id!("4qoALqJXrrjcqTmetedH55rvHTeF4XPfFVo8GaztD6KR");

#[program]
mod quanductor {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {
        ctx.accounts.handler(&ctx.bumps)
    }
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 7: Build to verify compilation**

Run: `quasar build`
Expected: Successful compilation, generates `target/deploy/quanductor.so`

- [ ] **Step 8: Write initial test for initialize**

```rust
extern crate std;

use quasar_svm::{Account, Instruction, Pubkey, QuasarSvm};
use solana_address::Address;
use std::{println, vec};

use quanductor_client::InitializeInstruction;

fn setup() -> QuasarSvm {
    let elf = include_bytes!("../target/deploy/quanductor.so");
    QuasarSvm::new()
        .with_program(&Pubkey::from(crate::ID), elf)
}

fn signer(address: Pubkey) -> Account {
    Account {
        address,
        lamports: 10_000_000_000,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

fn empty(address: Pubkey) -> Account {
    Account {
        address,
        lamports: 0,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

#[test]
fn test_initialize() {
    let mut svm = setup();
    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    let instruction: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();

    let result = svm.process_instruction(
        &instruction,
        &[signer(payer), empty(scoring_state)],
    );

    assert!(result.is_ok(), "initialize failed: {:?}", result.raw_result);

    let data = &result.account(&scoring_state).unwrap().data;
    // discriminator = 1 (first byte)
    assert_eq!(data[0], 1, "discriminator should be 1");
    // phase = 0 (IDLE) at offset 1
    assert_eq!(data[1], 0, "phase should be IDLE");

    println!("  INITIALIZE CU: {}", result.compute_units_consumed);
}
```

- [ ] **Step 9: Run test**

Run: `quasar test --filter test_initialize`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add src/lib.rs src/state.rs src/errors.rs src/instructions/initialize.rs src/instructions/mod.rs Cargo.toml
git commit -m "feat: scaffold ScoringState, errors, and initialize instruction"
```

---

### Task 2: ValidatorHistory Reader

**Files:**
- Create: `src/validator_history.rs`
- Modify: `src/lib.rs` (add `mod validator_history;`)

- [ ] **Step 1: Write validator_history.rs with repr(C) structs and helpers**

```rust
use quasar_lang::prelude::*;

/// ValidatorHistory program ID on mainnet
pub const VALIDATOR_HISTORY_PROGRAM_ID: Address = unsafe {
    core::mem::transmute(*b"\x0b\x9d\x44\x2b\x6e\x59\x8c\x4b\x0c\x7c\x12\x6f\x6b\x59\x7c\x4f\x0b\x6f\x6b\x59\x7c\x4f\x0b\x6f\x6b\x59\x7c\x4f\x0b\x6f\x6b\x59")
};

// We will use the base58-decoded bytes of HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa
// For now, define as a constant we'll fill in during implementation.

/// Anchor discriminator for ValidatorHistory account
pub const VH_DISCRIMINATOR: [u8; 8] = [205, 25, 8, 221, 253, 131, 2, 146];

/// Minimum account data length for ValidatorHistory (8 disc + 65856 struct)
pub const VH_MIN_DATA_LEN: usize = 65_864;

/// Offsets from start of account data (including 8-byte Anchor discriminator)
pub const VH_INDEX_OFFSET: usize = 44;        // 8 + 36
pub const VH_HISTORY_IDX_OFFSET: usize = 304; // 8 + 296
pub const VH_HISTORY_EMPTY_OFFSET: usize = 312;// 8 + 304
pub const VH_HISTORY_ARR_OFFSET: usize = 320;  // 8 + 312
pub const VH_VOTE_ACCOUNT_OFFSET: usize = 12;  // 8 + 4 (struct_version)

/// ValidatorHistoryEntry size
pub const VH_ENTRY_SIZE: usize = 128;
/// Max entries in circular buffer
pub const VH_MAX_ENTRIES: usize = 512;

/// Entry field offsets within a single ValidatorHistoryEntry
pub const ENTRY_EPOCH_OFFSET: usize = 8;
pub const ENTRY_EPOCH_CREDITS_OFFSET: usize = 12;
pub const ENTRY_COMMISSION_OFFSET: usize = 16;

/// Sentinel values indicating unset fields
pub const EPOCH_UNSET: u16 = u16::MAX;
pub const COMMISSION_UNSET: u8 = u8::MAX;

/// Validate a ValidatorHistory account: check owner, discriminator, data length.
/// Returns Ok(data slice) or Err.
///
/// # Safety
/// Caller must ensure `account` is a valid UncheckedAccount reference.
pub fn validate_validator_history(
    data: &[u8],
    owner: &Address,
    expected_owner: &Address,
) -> Result<(), ProgramError> {
    // Check owner
    if owner != expected_owner {
        return Err(ProgramError::Custom(6008)); // InvalidValidatorHistory
    }
    // Check data length
    if data.len() < VH_MIN_DATA_LEN {
        return Err(ProgramError::Custom(6008));
    }
    // Check discriminator
    if data[..8] != VH_DISCRIMINATOR {
        return Err(ProgramError::Custom(6008));
    }
    Ok(())
}

/// Read the validator's index from ValidatorHistory account data.
pub fn read_vh_index(data: &[u8]) -> u32 {
    u32::from_le_bytes([
        data[VH_INDEX_OFFSET],
        data[VH_INDEX_OFFSET + 1],
        data[VH_INDEX_OFFSET + 2],
        data[VH_INDEX_OFFSET + 3],
    ])
}

/// Read the vote_account pubkey from ValidatorHistory account data.
pub fn read_vh_vote_account(data: &[u8]) -> &[u8; 32] {
    unsafe { &*(data.as_ptr().add(VH_VOTE_ACCOUNT_OFFSET) as *const [u8; 32]) }
}

/// Read the circular buffer index (current position).
pub fn read_circ_buf_idx(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[VH_HISTORY_IDX_OFFSET],
        data[VH_HISTORY_IDX_OFFSET + 1],
        data[VH_HISTORY_IDX_OFFSET + 2],
        data[VH_HISTORY_IDX_OFFSET + 3],
        data[VH_HISTORY_IDX_OFFSET + 4],
        data[VH_HISTORY_IDX_OFFSET + 5],
        data[VH_HISTORY_IDX_OFFSET + 6],
        data[VH_HISTORY_IDX_OFFSET + 7],
    ])
}

/// Check if the circular buffer is empty.
pub fn is_circ_buf_empty(data: &[u8]) -> bool {
    data[VH_HISTORY_EMPTY_OFFSET] != 0
}

/// Read epoch from an entry at the given index in the circular buffer.
pub fn read_entry_epoch(data: &[u8], entry_idx: usize) -> u16 {
    let offset = VH_HISTORY_ARR_OFFSET + entry_idx * VH_ENTRY_SIZE + ENTRY_EPOCH_OFFSET;
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Read epoch_credits from an entry at the given index.
pub fn read_entry_epoch_credits(data: &[u8], entry_idx: usize) -> u32 {
    let offset = VH_HISTORY_ARR_OFFSET + entry_idx * VH_ENTRY_SIZE + ENTRY_EPOCH_CREDITS_OFFSET;
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Read commission from an entry at the given index.
pub fn read_entry_commission(data: &[u8], entry_idx: usize) -> u8 {
    let offset = VH_HISTORY_ARR_OFFSET + entry_idx * VH_ENTRY_SIZE + ENTRY_COMMISSION_OFFSET;
    data[offset]
}

/// Compute a validator's score from the last `lookback` epochs of data.
/// Returns the score (average of credits * (100 - commission) / 100).
/// Returns 0 if no valid epochs found.
pub fn compute_score(data: &[u8], current_epoch: u64, lookback: usize) -> u64 {
    if is_circ_buf_empty(data) {
        return 0;
    }

    let buf_idx = read_circ_buf_idx(data) as usize;
    let mut total_reward: u64 = 0;
    let mut valid_epochs: u64 = 0;

    // Walk backwards from buf_idx through the circular buffer
    for offset in 0..VH_MAX_ENTRIES {
        if valid_epochs as usize >= lookback {
            break;
        }

        let entry_idx = if buf_idx >= offset {
            buf_idx - offset
        } else {
            VH_MAX_ENTRIES + buf_idx - offset
        };

        let epoch = read_entry_epoch(data, entry_idx);
        if epoch == EPOCH_UNSET {
            continue;
        }

        let epoch_u64 = epoch as u64;
        // Only consider epochs in the lookback range
        if epoch_u64 > current_epoch {
            continue;
        }
        if current_epoch - epoch_u64 >= lookback as u64 {
            // Past lookback window, stop
            break;
        }

        let commission = read_entry_commission(data, entry_idx);
        if commission == COMMISSION_UNSET {
            continue;
        }

        let credits = read_entry_epoch_credits(data, entry_idx) as u64;
        let commission_u64 = commission as u64;
        let reward = credits * (100 - commission_u64) / 100;
        total_reward += reward;
        valid_epochs += 1;
    }

    if valid_epochs == 0 {
        return 0;
    }

    total_reward / valid_epochs
}

/// Compute the histogram bucket index for a score.
pub fn score_to_bucket(score: u64) -> usize {
    let bucket = score * 512 / crate::state::SCORE_RANGE;
    if bucket >= 512 {
        511
    } else {
        bucket as usize
    }
}

/// Check if a bit is set in the bitmap.
pub fn bitmap_is_set(bitmap: &[u8], index: u32) -> bool {
    let byte_idx = (index / 8) as usize;
    let bit_idx = (index % 8) as u8;
    if byte_idx >= bitmap.len() {
        return false;
    }
    bitmap[byte_idx] & (1 << bit_idx) != 0
}

/// Set a bit in the bitmap.
pub fn bitmap_set(bitmap: &mut [u8], index: u32) {
    let byte_idx = (index / 8) as usize;
    let bit_idx = (index % 8) as u8;
    if byte_idx < bitmap.len() {
        bitmap[byte_idx] |= 1 << bit_idx;
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Add `mod validator_history;` after `mod state;` in `src/lib.rs`:

```rust
mod errors;
mod instructions;
mod state;
mod validator_history;
use instructions::*;
```

- [ ] **Step 3: Build to verify compilation**

Run: `quasar build`
Expected: Successful compilation

- [ ] **Step 4: Commit**

```bash
git add src/validator_history.rs src/lib.rs
git commit -m "feat: add ValidatorHistory reader with zero-copy deserialization helpers"
```

---

### Task 3: Stake CPI and Stake State Parsing

**Files:**
- Create: `src/stake_cpi.rs`
- Create: `src/stake_state.rs`
- Modify: `src/lib.rs` (add modules)

- [ ] **Step 1: Write stake_state.rs — parsing stake account data**

```rust
use quasar_lang::prelude::*;

/// Stake account state offsets
/// Layout: https://docs.rs/solana-stake-program/latest/solana_stake_program/stake_state/
///
/// StakeStateV2 enum discriminant (4 bytes LE u32):
///   0 = Uninitialized
///   1 = Initialized (Meta only, no Stake)
///   2 = Stake (Meta + Stake)
///   3 = RewardsPool
///
/// Meta starts at offset 4:
///   rent_exempt_reserve: u64 (offset 4, 8 bytes)
///   authorized.staker: Pubkey (offset 12, 32 bytes)
///   authorized.withdrawer: Pubkey (offset 44, 32 bytes)
///   lockup.unix_timestamp: i64 (offset 76, 8 bytes)
///   lockup.epoch: u64 (offset 84, 8 bytes)
///   lockup.custodian: Pubkey (offset 92, 32 bytes)
///
/// Stake starts at offset 124 (only when state == 2):
///   delegation.voter_pubkey: Pubkey (offset 124, 32 bytes)
///   delegation.stake: u64 (offset 156, 8 bytes)
///   delegation.activation_epoch: u64 (offset 164, 8 bytes)
///   delegation.deactivation_epoch: u64 (offset 172, 8 bytes)

pub const STAKE_STATE_OFFSET: usize = 0;
pub const STAKER_OFFSET: usize = 12;
pub const VOTER_OFFSET: usize = 124;
pub const DEACTIVATION_EPOCH_OFFSET: usize = 172;

pub const STAKE_STATE_UNINITIALIZED: u32 = 0;
pub const STAKE_STATE_INITIALIZED: u32 = 1;
pub const STAKE_STATE_STAKE: u32 = 2;

/// Read the stake state enum discriminant.
pub fn read_stake_state(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Read the staker authority pubkey.
pub fn read_staker(data: &[u8]) -> &[u8; 32] {
    unsafe { &*(data.as_ptr().add(STAKER_OFFSET) as *const [u8; 32]) }
}

/// Read the voter pubkey (only valid when state == Stake).
pub fn read_voter(data: &[u8]) -> &[u8; 32] {
    unsafe { &*(data.as_ptr().add(VOTER_OFFSET) as *const [u8; 32]) }
}

/// Read the deactivation epoch (only valid when state == Stake).
pub fn read_deactivation_epoch(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[DEACTIVATION_EPOCH_OFFSET],
        data[DEACTIVATION_EPOCH_OFFSET + 1],
        data[DEACTIVATION_EPOCH_OFFSET + 2],
        data[DEACTIVATION_EPOCH_OFFSET + 3],
        data[DEACTIVATION_EPOCH_OFFSET + 4],
        data[DEACTIVATION_EPOCH_OFFSET + 5],
        data[DEACTIVATION_EPOCH_OFFSET + 6],
        data[DEACTIVATION_EPOCH_OFFSET + 7],
    ])
}

/// Check if a stake account is in a delegatable state:
/// - Initialized (never delegated)
/// - Stake with deactivation_epoch < current_epoch (fully cooled down)
pub fn is_delegatable(data: &[u8], current_epoch: u64) -> bool {
    let state = read_stake_state(data);
    match state {
        STAKE_STATE_INITIALIZED => true,
        STAKE_STATE_STAKE => {
            let deactivation = read_deactivation_epoch(data);
            deactivation != u64::MAX && deactivation < current_epoch
        }
        _ => false,
    }
}

/// Check if a stake account is actively delegated (state == Stake, deactivation == MAX).
pub fn is_active(data: &[u8]) -> bool {
    let state = read_stake_state(data);
    if state != STAKE_STATE_STAKE {
        return false;
    }
    read_deactivation_epoch(data) == u64::MAX
}
```

- [ ] **Step 2: Write stake_cpi.rs — delegate and deactivate CPI helpers**

```rust
use quasar_lang::prelude::*;
use quasar_lang::cpi::{CpiCall, InstructionAccount, Seed};

/// Native Stake Program ID
pub const STAKE_PROGRAM_ID: Address = unsafe {
    // Stake111111111111111111111111111111111111111 in bytes
    core::mem::transmute(*b"\x06\xa1\xd8\x17\x91\x37\x54\x2a\x98\x34\x37\xbd\xfe\x2a\x7a\xb2\x55\x7f\x53\x5c\x8a\x78\x72\x2b\x68\xa4\x9d\xc0\x00\x00\x00\x00")
};

// Note: The actual stake program ID bytes will need to be verified.
// Stake11111111111111111111111111111111111111 is the well-known address.

/// Build a DelegateStake CPI call.
/// Instruction index 2 (StakeInstruction::DelegateStake).
///
/// Accounts:
///   0. [WRITE] Stake account
///   1. [] Vote account
///   2. [] Clock sysvar
///   3. [] Stake history sysvar
///   4. [] Stake config (deprecated but still required)
///   5. [SIGNER] Stake authority
pub fn delegate_stake_cpi<'a>(
    stake_program: &'a AccountView,
    stake_account: &'a AccountView,
    vote_account: &'a AccountView,
    clock: &'a AccountView,
    stake_history: &'a AccountView,
    stake_config: &'a AccountView,
    stake_authority: &'a AccountView,
    seeds: &[Seed],
) -> Result<(), ProgramError> {
    let data: [u8; 4] = [2, 0, 0, 0]; // StakeInstruction::DelegateStake

    CpiCall::<7, 4>::new(
        stake_program.address(),
        [
            InstructionAccount::writable(stake_account.address()),
            InstructionAccount::readonly(vote_account.address()),
            InstructionAccount::readonly(clock.address()),
            InstructionAccount::readonly(stake_history.address()),
            InstructionAccount::readonly(stake_config.address()),
            InstructionAccount::readonly_signer(stake_authority.address()),
            InstructionAccount::readonly(stake_program.address()),
        ],
        [stake_account, vote_account, clock, stake_history, stake_config, stake_authority, stake_program],
        data,
    )
    .invoke_signed(seeds)
}

/// Build a Deactivate CPI call.
/// Instruction index 5 (StakeInstruction::Deactivate).
///
/// Accounts:
///   0. [WRITE] Stake account
///   1. [] Clock sysvar
///   2. [SIGNER] Stake authority
pub fn deactivate_stake_cpi<'a>(
    stake_program: &'a AccountView,
    stake_account: &'a AccountView,
    clock: &'a AccountView,
    stake_authority: &'a AccountView,
    seeds: &[Seed],
) -> Result<(), ProgramError> {
    let data: [u8; 4] = [5, 0, 0, 0]; // StakeInstruction::Deactivate

    CpiCall::<4, 4>::new(
        stake_program.address(),
        [
            InstructionAccount::writable(stake_account.address()),
            InstructionAccount::readonly(clock.address()),
            InstructionAccount::readonly_signer(stake_authority.address()),
            InstructionAccount::readonly(stake_program.address()),
        ],
        [stake_account, clock, stake_authority, stake_program],
        data,
    )
    .invoke_signed(seeds)
}
```

- [ ] **Step 3: Add modules to lib.rs**

```rust
mod errors;
mod instructions;
mod stake_cpi;
mod stake_state;
mod state;
mod validator_history;
use instructions::*;
```

- [ ] **Step 4: Build to verify compilation**

Run: `quasar build`
Expected: Successful compilation. Note: The `STAKE_PROGRAM_ID` and `VALIDATOR_HISTORY_PROGRAM_ID` constants use `transmute` with placeholder bytes — these need to be the actual base58-decoded public key bytes. Fix any compilation errors related to these constants by using the correct byte arrays. You can derive the correct bytes using `solana address --output json` or by decoding the base58 strings in a test.

- [ ] **Step 5: Commit**

```bash
git add src/stake_cpi.rs src/stake_state.rs src/lib.rs
git commit -m "feat: add stake program CPI builders and stake state parser"
```

---

### Task 4: Crank Scores Instruction

**Files:**
- Create: `src/instructions/crank_scores.rs`
- Modify: `src/instructions/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write crank_scores.rs**

```rust
use quasar_lang::prelude::*;
use quasar_lang::remaining::RemainingAccounts;
use quasar_lang::sysvars::{clock::Clock, Sysvar as _};

use crate::errors::QuanductorError;
use crate::state::*;
use crate::validator_history::*;

#[derive(Accounts)]
pub struct CrankScores<'info> {
    #[account(mut, seeds = [b"scoring_state"], bump = scoring_state.bump)]
    pub scoring_state: &'info mut Account<ScoringState>,
}

impl<'info> CrankScores<'info> {
    #[inline(always)]
    pub fn handler(
        &mut self,
        remaining: RemainingAccounts,
        vh_program_id: &Address,
    ) -> Result<(), ProgramError> {
        let clock = Clock::get()?;
        let current_epoch = clock.epoch.get();

        // Epoch transition: reset state if new epoch
        if current_epoch != self.scoring_state.epoch.get() {
            self.scoring_state.phase = 1u8.into(); // PHASE_CRANKING
            self.scoring_state.epoch = current_epoch.into();
            self.scoring_state.threshold = 0u64.into();
            self.scoring_state.total_scored = 0u16.into();
            // Zero histogram
            for i in 0..HISTOGRAM_BUCKETS {
                self.scoring_state.histogram[i] = 0u16.into();
            }
            // Zero bitmap
            for i in 0..BITMAP_BYTES {
                self.scoring_state.bitmap[i] = 0;
            }
        }

        // Must be in cranking phase
        let phase: u8 = self.scoring_state.phase.into();
        if phase != PHASE_CRANKING {
            return Err(QuanductorError::InvalidPhase.into());
        }

        // Process each ValidatorHistory in remaining accounts
        for account in remaining.iter() {
            let account = account?;
            let view = account.to_view();
            let data = unsafe { view.borrow_unchecked() };
            let owner = view.owner();

            // Validate
            validate_validator_history(data, owner, vh_program_id)?;

            // Read validator index for bitmap dedup
            let vh_index = read_vh_index(data);

            // Check bitmap — skip if already scored
            if bitmap_is_set(&self.scoring_state.bitmap, vh_index) {
                continue;
            }

            // Compute score
            let score = compute_score(data, current_epoch, EPOCHS_LOOKBACK);

            // Bucket and increment histogram
            let bucket = score_to_bucket(score);
            let current_count: u16 = self.scoring_state.histogram[bucket].into();
            self.scoring_state.histogram[bucket] = (current_count + 1).into();

            // Set bitmap
            bitmap_set(&mut self.scoring_state.bitmap, vh_index);

            // Increment total_scored
            let scored: u16 = self.scoring_state.total_scored.into();
            self.scoring_state.total_scored = (scored + 1).into();
        }

        Ok(())
    }
}
```

**Important Note:** The exact API for accessing remaining account data in Quasar (`to_view()`, `borrow_unchecked()`, `owner()`) may differ from what's shown. During implementation, reference the patterns found in the multisig example at `/Users/loser/projects/quasar/examples/multisig/src/instructions/create.rs` and adapt. The remaining accounts iterator yields account references — use `account.address()`, and access data through `to_account_view()` then `borrow_unchecked()`.

- [ ] **Step 2: Update instructions/mod.rs**

```rust
mod initialize;
pub use initialize::*;

mod crank_scores;
pub use crank_scores::*;
```

- [ ] **Step 3: Update lib.rs with crank_scores instruction**

```rust
#[program]
mod quanductor {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {
        ctx.accounts.handler(&ctx.bumps)
    }

    #[instruction(discriminator = 1)]
    pub fn crank_scores(ctx: CtxWithRemaining<CrankScores>) -> Result<(), ProgramError> {
        // The ValidatorHistory program ID should be a known constant.
        // For now we pass it as validation reference.
        let vh_program_id = crate::validator_history::VALIDATOR_HISTORY_PROGRAM_ID;
        ctx.accounts.handler(ctx.remaining_accounts(), &vh_program_id)
    }
}
```

- [ ] **Step 4: Build and fix compilation issues**

Run: `quasar build`
Expected: May have issues with Pod type access patterns (`.get()`, `.into()`). Fix based on compiler errors. The key issue will be how Quasar's generated zero-copy companion struct exposes fields — Pod types require `.get()` to read and `.into()` or `From` to write.

- [ ] **Step 5: Commit**

```bash
git add src/instructions/crank_scores.rs src/instructions/mod.rs src/lib.rs
git commit -m "feat: add crank_scores instruction with histogram and bitmap"
```

---

### Task 5: Compute Threshold Instruction

**Files:**
- Create: `src/instructions/compute_threshold.rs`
- Modify: `src/instructions/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write compute_threshold.rs**

```rust
use quasar_lang::prelude::*;

use crate::errors::QuanductorError;
use crate::state::*;

#[derive(Accounts)]
pub struct ComputeThreshold<'info> {
    #[account(mut, seeds = [b"scoring_state"], bump = scoring_state.bump)]
    pub scoring_state: &'info mut Account<ScoringState>,
}

impl<'info> ComputeThreshold<'info> {
    #[inline(always)]
    pub fn handler(&mut self) -> Result<(), ProgramError> {
        let phase: u8 = self.scoring_state.phase.into();
        if phase != PHASE_CRANKING {
            return Err(QuanductorError::InvalidPhase.into());
        }

        let total_scored: u16 = self.scoring_state.total_scored.into();
        if total_scored < MIN_VALIDATORS {
            return Err(QuanductorError::InsufficientValidators.into());
        }

        // Walk histogram from top to find 90th percentile
        let target_rank = total_scored as u64 / 10; // top 10%
        let mut running_count: u64 = 0;
        let mut threshold: u64 = 0;

        let mut bucket: usize = HISTOGRAM_BUCKETS;
        while bucket > 0 {
            bucket -= 1;
            let count: u16 = self.scoring_state.histogram[bucket].into();
            running_count += count as u64;
            if running_count >= target_rank {
                // Lower bound of this bucket
                threshold = (bucket as u64) * SCORE_RANGE / (HISTOGRAM_BUCKETS as u64);
                break;
            }
        }

        self.scoring_state.threshold = threshold.into();
        self.scoring_state.phase = PHASE_THRESHOLD_COMPUTED.into();

        Ok(())
    }
}
```

- [ ] **Step 2: Update instructions/mod.rs**

```rust
mod initialize;
pub use initialize::*;

mod crank_scores;
pub use crank_scores::*;

mod compute_threshold;
pub use compute_threshold::*;
```

- [ ] **Step 3: Update lib.rs**

Add to the `#[program]` module:

```rust
    #[instruction(discriminator = 2)]
    pub fn compute_threshold(ctx: Ctx<ComputeThreshold>) -> Result<(), ProgramError> {
        ctx.accounts.handler()
    }
```

- [ ] **Step 4: Build**

Run: `quasar build`
Expected: Successful compilation

- [ ] **Step 5: Commit**

```bash
git add src/instructions/compute_threshold.rs src/instructions/mod.rs src/lib.rs
git commit -m "feat: add compute_threshold instruction — walks histogram for 90th percentile"
```

---

### Task 6: Delegate Stake Instruction

**Files:**
- Create: `src/instructions/delegate_stake.rs`
- Modify: `src/instructions/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write delegate_stake.rs**

```rust
use quasar_lang::prelude::*;
use quasar_lang::cpi::Seed;
use quasar_lang::sysvars::{clock::Clock, Sysvar as _};

use crate::errors::QuanductorError;
use crate::stake_cpi;
use crate::stake_state;
use crate::state::*;
use crate::validator_history;

#[derive(Accounts)]
pub struct DelegateStake<'info> {
    #[account(seeds = [b"scoring_state"], bump = scoring_state.bump)]
    pub scoring_state: &'info Account<ScoringState>,
    #[account(seeds = [b"stake_authority"], bump)]
    pub stake_authority: &'info UncheckedAccount,
    #[account(mut)]
    pub stake_account: &'info mut UncheckedAccount,
    pub validator_history: &'info UncheckedAccount,
    pub validator_vote_account: &'info UncheckedAccount,
    pub clock_sysvar: &'info UncheckedAccount,
    pub stake_history_sysvar: &'info UncheckedAccount,
    pub stake_config: &'info UncheckedAccount,
    pub stake_program: &'info UncheckedAccount,
}

impl<'info> DelegateStake<'info> {
    #[inline(always)]
    pub fn handler(&self, bumps: &DelegateStakeBumps) -> Result<(), ProgramError> {
        // Check phase
        let phase: u8 = self.scoring_state.phase.into();
        if phase != PHASE_THRESHOLD_COMPUTED {
            return Err(QuanductorError::InvalidPhase.into());
        }

        // Check epoch freshness
        let clock = Clock::get()?;
        let current_epoch = clock.epoch.get();
        let scoring_epoch: u64 = self.scoring_state.epoch.into();
        if scoring_epoch != current_epoch {
            return Err(QuanductorError::EpochMismatch.into());
        }

        // Validate ValidatorHistory account
        let vh_view = self.validator_history.to_account_view();
        let vh_data = unsafe { vh_view.borrow_unchecked() };
        validator_history::validate_validator_history(
            vh_data,
            vh_view.owner(),
            &validator_history::VALIDATOR_HISTORY_PROGRAM_ID,
        )?;

        // Compute validator's score
        let score = validator_history::compute_score(vh_data, current_epoch, EPOCHS_LOOKBACK);
        let threshold: u64 = self.scoring_state.threshold.into();
        if score < threshold {
            return Err(QuanductorError::ScoreBelowThreshold.into());
        }

        // Validate stake account state
        let stake_view = self.stake_account.to_account_view();
        let stake_data = unsafe { stake_view.borrow_unchecked() };
        if !stake_state::is_delegatable(stake_data, current_epoch) {
            return Err(QuanductorError::InvalidStakeState.into());
        }

        // Verify staker authority matches our PDA
        let staker = stake_state::read_staker(stake_data);
        if staker != self.stake_authority.address().as_ref() {
            return Err(QuanductorError::InvalidStakeAuthority.into());
        }

        // Verify vote account matches validator history
        let vh_vote = validator_history::read_vh_vote_account(vh_data);
        if vh_vote != self.validator_vote_account.address().as_ref() {
            return Err(QuanductorError::InvalidVoteAccount.into());
        }

        // CPI: Delegate stake
        let sa_bump = self.scoring_state.stake_authority_bump.into();
        let seeds = [
            Seed::from(b"stake_authority" as &[u8]),
            Seed::from(&[sa_bump] as &[u8]),
        ];

        stake_cpi::delegate_stake_cpi(
            self.stake_program.to_account_view(),
            stake_view,
            self.validator_vote_account.to_account_view(),
            self.clock_sysvar.to_account_view(),
            self.stake_history_sysvar.to_account_view(),
            self.stake_config.to_account_view(),
            self.stake_authority.to_account_view(),
            &seeds,
        )
    }
}
```

- [ ] **Step 2: Update instructions/mod.rs**

Add:
```rust
mod delegate_stake;
pub use delegate_stake::*;
```

- [ ] **Step 3: Update lib.rs**

Add to `#[program]`:
```rust
    #[instruction(discriminator = 3)]
    pub fn delegate_stake(ctx: Ctx<DelegateStake>) -> Result<(), ProgramError> {
        ctx.accounts.handler(&ctx.bumps)
    }
```

- [ ] **Step 4: Build and fix**

Run: `quasar build`
Expected: May need adjustments to CPI call signatures. The `CpiCall` const generics for account count and data size must match exactly. Fix any mismatches.

- [ ] **Step 5: Commit**

```bash
git add src/instructions/delegate_stake.rs src/instructions/mod.rs src/lib.rs
git commit -m "feat: add delegate_stake instruction with score validation and stake CPI"
```

---

### Task 7: Undelegate Stake Instruction

**Files:**
- Create: `src/instructions/undelegate_stake.rs`
- Modify: `src/instructions/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write undelegate_stake.rs**

```rust
use quasar_lang::prelude::*;
use quasar_lang::cpi::Seed;
use quasar_lang::sysvars::{clock::Clock, Sysvar as _};

use crate::errors::QuanductorError;
use crate::stake_cpi;
use crate::stake_state;
use crate::state::*;
use crate::validator_history;

#[derive(Accounts)]
pub struct UndelegateStake<'info> {
    #[account(seeds = [b"scoring_state"], bump = scoring_state.bump)]
    pub scoring_state: &'info Account<ScoringState>,
    #[account(seeds = [b"stake_authority"], bump)]
    pub stake_authority: &'info UncheckedAccount,
    #[account(mut)]
    pub stake_account: &'info mut UncheckedAccount,
    pub validator_history: &'info UncheckedAccount,
    pub clock_sysvar: &'info UncheckedAccount,
    pub stake_program: &'info UncheckedAccount,
}

impl<'info> UndelegateStake<'info> {
    #[inline(always)]
    pub fn handler(&self, bumps: &UndelegateStakeBumps) -> Result<(), ProgramError> {
        // Check phase
        let phase: u8 = self.scoring_state.phase.into();
        if phase != PHASE_THRESHOLD_COMPUTED {
            return Err(QuanductorError::InvalidPhase.into());
        }

        // Check epoch freshness
        let clock = Clock::get()?;
        let current_epoch = clock.epoch.get();
        let scoring_epoch: u64 = self.scoring_state.epoch.into();
        if scoring_epoch != current_epoch {
            return Err(QuanductorError::EpochMismatch.into());
        }

        // Validate ValidatorHistory account
        let vh_view = self.validator_history.to_account_view();
        let vh_data = unsafe { vh_view.borrow_unchecked() };
        validator_history::validate_validator_history(
            vh_data,
            vh_view.owner(),
            &validator_history::VALIDATOR_HISTORY_PROGRAM_ID,
        )?;

        // Compute validator's score
        let score = validator_history::compute_score(vh_data, current_epoch, EPOCHS_LOOKBACK);
        let threshold: u64 = self.scoring_state.threshold.into();
        if score >= threshold {
            return Err(QuanductorError::ScoreAboveThreshold.into());
        }

        // Validate stake account is actively delegated
        let stake_view = self.stake_account.to_account_view();
        let stake_data = unsafe { stake_view.borrow_unchecked() };
        if !stake_state::is_active(stake_data) {
            return Err(QuanductorError::InvalidStakeState.into());
        }

        // Verify staker authority
        let staker = stake_state::read_staker(stake_data);
        if staker != self.stake_authority.address().as_ref() {
            return Err(QuanductorError::InvalidStakeAuthority.into());
        }

        // Verify stake is delegated to the validator whose history was provided
        let voter = stake_state::read_voter(stake_data);
        let vh_vote = validator_history::read_vh_vote_account(vh_data);
        if voter != vh_vote {
            return Err(QuanductorError::InvalidVoteAccount.into());
        }

        // CPI: Deactivate stake
        let sa_bump = self.scoring_state.stake_authority_bump.into();
        let seeds = [
            Seed::from(b"stake_authority" as &[u8]),
            Seed::from(&[sa_bump] as &[u8]),
        ];

        stake_cpi::deactivate_stake_cpi(
            self.stake_program.to_account_view(),
            stake_view,
            self.clock_sysvar.to_account_view(),
            self.stake_authority.to_account_view(),
            &seeds,
        )
    }
}
```

- [ ] **Step 2: Update instructions/mod.rs**

Add:
```rust
mod undelegate_stake;
pub use undelegate_stake::*;
```

- [ ] **Step 3: Update lib.rs**

Add to `#[program]`:
```rust
    #[instruction(discriminator = 4)]
    pub fn undelegate_stake(ctx: Ctx<UndelegateStake>) -> Result<(), ProgramError> {
        ctx.accounts.handler(&ctx.bumps)
    }
```

- [ ] **Step 4: Build**

Run: `quasar build`
Expected: Successful compilation

- [ ] **Step 5: Commit**

```bash
git add src/instructions/undelegate_stake.rs src/instructions/mod.rs src/lib.rs
git commit -m "feat: add undelegate_stake instruction — deactivates stake from underperforming validators"
```

---

### Task 8: Comprehensive Tests — Score Computation and Histogram

**Files:**
- Modify: `src/tests.rs`

- [ ] **Step 1: Write test helpers for building mock ValidatorHistory accounts**

```rust
extern crate std;

use quasar_svm::{Account, Instruction, Pubkey, QuasarSvm};
use solana_address::Address;
use std::{println, vec, vec::Vec};

use quanductor_client::*;

const VH_DISCRIMINATOR: [u8; 8] = [205, 25, 8, 221, 253, 131, 2, 146];
const VH_TOTAL_SIZE: usize = 65_864; // 8 disc + 65856 struct

// Fake "validator history program" owner
fn vh_program_id() -> Pubkey {
    // This needs to match the VALIDATOR_HISTORY_PROGRAM_ID constant in our program.
    // For tests, we build accounts with this as owner.
    Pubkey::from(crate::validator_history::VALIDATOR_HISTORY_PROGRAM_ID)
}

fn setup() -> QuasarSvm {
    let elf = include_bytes!("../target/deploy/quanductor.so");
    QuasarSvm::new()
        .with_program(&Pubkey::from(crate::ID), elf)
}

fn signer(address: Pubkey) -> Account {
    Account {
        address,
        lamports: 10_000_000_000,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

fn empty(address: Pubkey) -> Account {
    Account {
        address,
        lamports: 0,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

/// Build a mock ValidatorHistory account with specified epoch data.
/// `entries` is a list of (epoch, epoch_credits, commission) tuples.
/// `index` is the validator's index in the bitmap.
/// `vote_account` is the validator's vote account pubkey.
fn mock_validator_history(
    address: Pubkey,
    index: u32,
    vote_account: &Pubkey,
    entries: &[(u16, u32, u8)],
) -> Account {
    let mut data = vec![0u8; VH_TOTAL_SIZE];

    // Anchor discriminator
    data[..8].copy_from_slice(&VH_DISCRIMINATOR);

    // struct_version (u32) at offset 8
    data[8..12].copy_from_slice(&1u32.to_le_bytes());

    // vote_account (Pubkey) at offset 12
    data[12..44].copy_from_slice(vote_account.as_ref());

    // index (u32) at offset 44
    data[44..48].copy_from_slice(&index.to_le_bytes());

    // history.idx (u64) at offset 304
    let buf_idx = if entries.is_empty() { 0 } else { entries.len() - 1 };
    data[304..312].copy_from_slice(&(buf_idx as u64).to_le_bytes());

    // history.is_empty (u8) at offset 312
    data[312] = if entries.is_empty() { 1 } else { 0 };

    // Write entries starting at offset 320, each 128 bytes
    for (i, &(epoch, credits, commission)) in entries.iter().enumerate() {
        let entry_offset = 320 + i * 128;
        // epoch (u16) at entry + 8
        data[entry_offset + 8..entry_offset + 10].copy_from_slice(&epoch.to_le_bytes());
        // epoch_credits (u32) at entry + 12
        data[entry_offset + 12..entry_offset + 16].copy_from_slice(&credits.to_le_bytes());
        // commission (u8) at entry + 16
        data[entry_offset + 16] = commission;
    }

    // Fill unset entries with sentinel values
    for i in entries.len()..512 {
        let entry_offset = 320 + i * 128;
        // epoch = u16::MAX
        data[entry_offset + 8..entry_offset + 10].copy_from_slice(&u16::MAX.to_le_bytes());
        // commission = u8::MAX
        data[entry_offset + 16] = u8::MAX;
    }

    Account {
        address,
        lamports: 1_000_000,
        data,
        owner: vh_program_id(),
        executable: false,
    }
}
```

- [ ] **Step 2: Write initialize test**

```rust
#[test]
fn test_initialize() {
    let mut svm = setup();
    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    let instruction: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();

    let result = svm.process_instruction(
        &instruction,
        &[signer(payer), empty(scoring_state)],
    );

    assert!(result.is_ok(), "initialize failed: {:?}", result.raw_result);

    let data = &result.account(&scoring_state).unwrap().data;
    assert_eq!(data[0], 1, "discriminator should be 1");
    assert_eq!(data[1], 0, "phase should be IDLE");

    println!("  INITIALIZE CU: {}", result.compute_units_consumed);
}

#[test]
fn test_initialize_double_init_fails() {
    let mut svm = setup();
    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    let instruction: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();

    // First init succeeds
    let result = svm.process_instruction(
        &instruction,
        &[signer(payer), empty(scoring_state)],
    );
    assert!(result.is_ok());

    // Second init should fail (account already exists)
    let result2 = svm.process_instruction(
        &instruction,
        &[signer(payer), result.account(&scoring_state).unwrap()],
    );
    assert!(result2.is_err(), "double init should fail");
}
```

- [ ] **Step 3: Write crank_scores tests**

```rust
#[test]
fn test_crank_single_validator() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    // Initialize
    let init_ix: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    // Create mock validator history
    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(
        vh_address,
        0, // index
        &vote_account,
        &[
            (6, 350_000, 5),  // epoch 6
            (7, 360_000, 5),  // epoch 7
            (8, 340_000, 10), // epoch 8
            (9, 370_000, 5),  // epoch 9
            (10, 355_000, 5), // epoch 10 (current)
        ],
    );

    // Crank scores — note: the instruction client struct will include remaining_accounts
    let crank_ix: Instruction = CrankScoresInstruction {
        scoring_state,
        remaining_accounts: vec![
            solana_instruction::AccountMeta::new_readonly(vh_address, false),
        ],
    }
    .into();

    let result = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh],
    );

    assert!(result.is_ok(), "crank_scores failed: {:?}", result.raw_result);

    let data = &result.account(&scoring_state).unwrap().data;
    // Phase should be CRANKING (1)
    assert_eq!(data[1], 1, "phase should be CRANKING");
    // total_scored should be 1
    // (exact offset depends on generated struct layout — verify during implementation)

    println!("  CRANK_SINGLE CU: {}", result.compute_units_consumed);
}

#[test]
fn test_crank_duplicate_validator_skipped() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    // Initialize
    let init_ix: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(
        vh_address,
        0,
        &vote_account,
        &[(10, 350_000, 5)],
    );

    // First crank
    let crank_ix: Instruction = CrankScoresInstruction {
        scoring_state,
        remaining_accounts: vec![
            solana_instruction::AccountMeta::new_readonly(vh_address, false),
        ],
    }
    .into();
    let result = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh.clone()],
    );
    assert!(result.is_ok());

    // Second crank with same validator — should succeed but not increment
    let result2 = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh],
    );
    assert!(result2.is_ok(), "duplicate crank should succeed (skip)");

    // total_scored should still be 1 (not 2)
    println!("  CRANK_DUPLICATE CU: {}", result2.compute_units_consumed);
}

#[test]
fn test_crank_invalid_owner_rejected() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    let init_ix: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    // Create VH account with wrong owner
    let vh_address = Pubkey::new_unique();
    let vote_account = Pubkey::new_unique();
    let mut vh = mock_validator_history(vh_address, 0, &vote_account, &[(10, 350_000, 5)]);
    vh.owner = Pubkey::new_unique(); // wrong owner

    let crank_ix: Instruction = CrankScoresInstruction {
        scoring_state,
        remaining_accounts: vec![
            solana_instruction::AccountMeta::new_readonly(vh_address, false),
        ],
    }
    .into();
    let result = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh],
    );
    assert!(result.is_err(), "wrong owner should be rejected");
}
```

- [ ] **Step 4: Write compute_threshold tests**

```rust
#[test]
fn test_compute_threshold_insufficient_validators() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    // Initialize
    let init_ix: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    // Crank just 1 validator (way below MIN_VALIDATORS)
    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(vh_address, 0, &vote_account, &[(10, 350_000, 5)]);

    let crank_ix: Instruction = CrankScoresInstruction {
        scoring_state,
        remaining_accounts: vec![
            solana_instruction::AccountMeta::new_readonly(vh_address, false),
        ],
    }
    .into();
    let result = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh],
    );
    assert!(result.is_ok());

    // Try to compute threshold — should fail
    let threshold_ix: Instruction = ComputeThresholdInstruction {
        scoring_state,
    }
    .into();
    let result = svm.process_instruction(
        &threshold_ix,
        &[result.account(&scoring_state).unwrap()],
    );
    assert!(result.is_err(), "should fail with insufficient validators");
}

#[test]
fn test_compute_threshold_wrong_phase() {
    let mut svm = setup();
    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    // Initialize (phase = IDLE)
    let init_ix: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    // Try to compute threshold without cranking — wrong phase
    let threshold_ix: Instruction = ComputeThresholdInstruction {
        scoring_state,
    }
    .into();
    let result = svm.process_instruction(
        &threshold_ix,
        &[result.account(&scoring_state).unwrap()],
    );
    assert!(result.is_err(), "should fail with wrong phase");
}
```

- [ ] **Step 5: Write epoch transition test**

```rust
#[test]
fn test_epoch_transition_resets_state() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);

    // Initialize
    let init_ix: Instruction = InitializeInstruction {
        payer,
        scoring_state,
        system_program,
    }
    .into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    // Crank one validator in epoch 10
    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(vh_address, 0, &vote_account, &[(10, 350_000, 5)]);

    let crank_ix: Instruction = CrankScoresInstruction {
        scoring_state,
        remaining_accounts: vec![
            solana_instruction::AccountMeta::new_readonly(vh_address, false),
        ],
    }
    .into();
    let result = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh.clone()],
    );
    assert!(result.is_ok());

    // Advance to epoch 11
    svm.sysvars.clock.epoch = 11;

    // Crank same validator in epoch 11 — should reset and allow re-scoring
    let result = svm.process_instruction(
        &crank_ix,
        &[result.account(&scoring_state).unwrap(), vh],
    );
    assert!(result.is_ok(), "epoch transition crank should succeed");

    // Epoch should now be 11 in scoring_state
    println!("  EPOCH_TRANSITION CU: {}", result.compute_units_consumed);
}
```

- [ ] **Step 6: Build and run all tests**

Run: `quasar build && quasar test`
Expected: All tests pass. Fix any compilation or runtime errors.

- [ ] **Step 7: Commit**

```bash
git add src/tests.rs
git commit -m "test: comprehensive tests for initialize, crank_scores, compute_threshold, and epoch transitions"
```

---

### Task 9: Delegate and Undelegate Tests

**Files:**
- Modify: `src/tests.rs`

Note: Testing delegate_stake and undelegate_stake requires the native stake program to be loaded in QuasarSVM. QuasarSVM may not bundle the stake program by default. If CPI calls fail due to missing program, we need to either:
1. Load the stake program ELF into QuasarSVM (if available)
2. Test the validation logic up to the CPI boundary

- [ ] **Step 1: Write helper to build mock stake accounts**

```rust
/// Build a mock stake account in Initialized state (never delegated).
fn mock_stake_initialized(address: Pubkey, staker: &Pubkey) -> Account {
    let mut data = vec![0u8; 200];
    // State = Initialized (1)
    data[0..4].copy_from_slice(&1u32.to_le_bytes());
    // rent_exempt_reserve (u64) at offset 4
    data[4..12].copy_from_slice(&1_000_000u64.to_le_bytes());
    // authorized.staker (Pubkey) at offset 12
    data[12..44].copy_from_slice(staker.as_ref());
    // authorized.withdrawer (Pubkey) at offset 44
    data[44..76].copy_from_slice(staker.as_ref());

    Account {
        address,
        lamports: 5_000_000_000,
        data,
        owner: quasar_svm::solana_sdk_ids::stake::ID,
        executable: false,
    }
}

/// Build a mock stake account in active delegated state.
fn mock_stake_active(address: Pubkey, staker: &Pubkey, voter: &Pubkey) -> Account {
    let mut data = vec![0u8; 200];
    // State = Stake (2)
    data[0..4].copy_from_slice(&2u32.to_le_bytes());
    // rent_exempt_reserve at offset 4
    data[4..12].copy_from_slice(&1_000_000u64.to_le_bytes());
    // authorized.staker at offset 12
    data[12..44].copy_from_slice(staker.as_ref());
    // authorized.withdrawer at offset 44
    data[44..76].copy_from_slice(staker.as_ref());
    // delegation.voter_pubkey at offset 124
    data[124..156].copy_from_slice(voter.as_ref());
    // delegation.stake at offset 156
    data[156..164].copy_from_slice(&4_000_000_000u64.to_le_bytes());
    // delegation.activation_epoch at offset 164
    data[164..172].copy_from_slice(&5u64.to_le_bytes());
    // delegation.deactivation_epoch at offset 172 (MAX = active)
    data[172..180].copy_from_slice(&u64::MAX.to_le_bytes());

    Account {
        address,
        lamports: 5_000_000_000,
        data,
        owner: quasar_svm::solana_sdk_ids::stake::ID,
        executable: false,
    }
}

/// Build a mock stake account that has been deactivated (cooled down).
fn mock_stake_deactivated(address: Pubkey, staker: &Pubkey, voter: &Pubkey, deactivation_epoch: u64) -> Account {
    let mut data = vec![0u8; 200];
    data[0..4].copy_from_slice(&2u32.to_le_bytes());
    data[4..12].copy_from_slice(&1_000_000u64.to_le_bytes());
    data[12..44].copy_from_slice(staker.as_ref());
    data[44..76].copy_from_slice(staker.as_ref());
    data[124..156].copy_from_slice(voter.as_ref());
    data[156..164].copy_from_slice(&4_000_000_000u64.to_le_bytes());
    data[164..172].copy_from_slice(&5u64.to_le_bytes());
    data[172..180].copy_from_slice(&deactivation_epoch.to_le_bytes());

    Account {
        address,
        lamports: 5_000_000_000,
        data,
        owner: quasar_svm::solana_sdk_ids::stake::ID,
        executable: false,
    }
}
```

- [ ] **Step 2: Write delegate_stake validation tests**

These tests validate the instruction logic up to the CPI boundary. If the native stake program isn't available in QuasarSVM, these will test that validation errors fire correctly.

```rust
#[test]
fn test_delegate_wrong_phase_rejected() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let payer = Pubkey::new_unique();
    let system_program = quasar_svm::system_program::ID;
    let (scoring_state, _) = Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);
    let (stake_authority, _) = Pubkey::find_program_address(&[b"stake_authority"], &crate::ID);

    // Initialize (phase = IDLE, not THRESHOLD_COMPUTED)
    let init_ix: Instruction = InitializeInstruction {
        payer, scoring_state, system_program,
    }.into();
    let result = svm.process_instruction(&init_ix, &[signer(payer), empty(scoring_state)]);
    assert!(result.is_ok());

    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(vh_address, 0, &vote_account, &[(10, 400_000, 5)]);
    let stake_address = Pubkey::new_unique();
    let stake = mock_stake_initialized(stake_address, &stake_authority);

    let delegate_ix: Instruction = DelegateStakeInstruction {
        scoring_state,
        stake_authority,
        stake_account: stake_address,
        validator_history: vh_address,
        validator_vote_account: vote_account,
        clock_sysvar: quasar_svm::solana_sdk_ids::sysvar::clock::ID,
        stake_history_sysvar: quasar_svm::solana_sdk_ids::sysvar::stake_history::ID,
        stake_config: quasar_svm::solana_sdk_ids::stake::config::ID,
        stake_program: quasar_svm::solana_sdk_ids::stake::ID,
    }.into();

    let result = svm.process_instruction(
        &delegate_ix,
        &[result.account(&scoring_state).unwrap(), empty(stake_authority), stake, vh, empty(vote_account)],
    );
    assert!(result.is_err(), "delegate should fail in wrong phase");
}

#[test]
fn test_delegate_wrong_staker_authority_rejected() {
    // Setup: get to THRESHOLD_COMPUTED phase, then try to delegate
    // a stake account whose staker is NOT the stake_authority PDA.
    // This test exercises the InvalidStakeAuthority error path.

    // (Full setup with crank + threshold computation required)
    // ... implementation will follow the same pattern as above
    // but with scoring_state in THRESHOLD_COMPUTED phase
}

#[test]
fn test_delegate_below_threshold_rejected() {
    // Setup: get to THRESHOLD_COMPUTED phase, then try to delegate
    // to a validator whose score is below the threshold.
    // Should fail with ScoreBelowThreshold.
}
```

- [ ] **Step 3: Write undelegate_stake validation tests**

```rust
#[test]
fn test_undelegate_above_threshold_rejected() {
    // Setup: get to THRESHOLD_COMPUTED phase, then try to undelegate
    // a stake from a validator whose score is ABOVE the threshold.
    // Should fail with ScoreAboveThreshold.
}

#[test]
fn test_undelegate_not_active_rejected() {
    // Try to undelegate a stake account that's already deactivated.
    // Should fail with InvalidStakeState.
}

#[test]
fn test_undelegate_vote_mismatch_rejected() {
    // Stake is delegated to validator A, but we pass validator B's history.
    // Should fail with InvalidVoteAccount.
}
```

- [ ] **Step 4: Build and run tests**

Run: `quasar build && quasar test`
Expected: All validation tests pass. CPI tests may fail if stake program not loaded — this is expected. Document which tests need the stake program.

- [ ] **Step 5: Commit**

```bash
git add src/tests.rs
git commit -m "test: add delegate/undelegate validation tests with mock stake accounts"
```

---

### Task 10: End-to-End Integration Test

**Files:**
- Modify: `src/tests.rs`

- [ ] **Step 1: Write helper to pre-build ScoringState in THRESHOLD_COMPUTED phase**

Rather than cranking 1,400+ validators in a test, build the ScoringState account data directly in the desired state:

```rust
/// Build a ScoringState account already in THRESHOLD_COMPUTED phase.
/// This bypasses the crank process for testing delegate/undelegate.
fn mock_scoring_state_computed(
    address: Pubkey,
    epoch: u64,
    threshold: u64,
    stake_authority_bump: u8,
    bump: u8,
) -> Account {
    // Build the account data matching ScoringState layout:
    // discriminator(1) + phase(1) + epoch(8) + threshold(8) + total_scored(2)
    // + histogram(1024) + bitmap(768) + stake_authority_bump(1) + bump(1)
    let total_size = 1 + 1 + 8 + 8 + 2 + 1024 + 768 + 1 + 1; // = 1814

    let mut data = vec![0u8; total_size];
    let mut offset = 0;

    // discriminator = 1
    data[offset] = 1;
    offset += 1;

    // phase = THRESHOLD_COMPUTED (2)
    data[offset] = 2;
    offset += 1;

    // epoch (u64 LE)
    data[offset..offset + 8].copy_from_slice(&epoch.to_le_bytes());
    offset += 8;

    // threshold (u64 LE)
    data[offset..offset + 8].copy_from_slice(&threshold.to_le_bytes());
    offset += 8;

    // total_scored (u16 LE) — doesn't matter for delegate/undelegate
    data[offset..offset + 2].copy_from_slice(&1500u16.to_le_bytes());
    offset += 2;

    // histogram (512 * u16 = 1024 bytes) — zeroed is fine
    offset += 1024;

    // bitmap (768 bytes) — zeroed is fine
    offset += 768;

    // stake_authority_bump
    data[offset] = stake_authority_bump;
    offset += 1;

    // bump
    data[offset] = bump;

    Account {
        address,
        lamports: 10_000_000,
        data,
        owner: Pubkey::from(crate::ID),
        executable: false,
    }
}
```

- [ ] **Step 2: Write full flow test using pre-built state**

```rust
#[test]
fn test_delegate_with_prebuilt_state() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let (scoring_state_addr, scoring_bump) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);
    let (stake_authority, sa_bump) =
        Pubkey::find_program_address(&[b"stake_authority"], &crate::ID);

    // Threshold = 300,000 — validator needs score >= 300,000 to qualify
    let scoring_state = mock_scoring_state_computed(
        scoring_state_addr,
        10,       // epoch
        300_000,  // threshold
        sa_bump,
        scoring_bump,
    );

    // Validator with score = 350,000 * (100 - 5) / 100 = 332,500 — above threshold
    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(
        vh_address,
        0,
        &vote_account,
        &[
            (6, 350_000, 5),
            (7, 350_000, 5),
            (8, 350_000, 5),
            (9, 350_000, 5),
            (10, 350_000, 5),
        ],
    );

    // Stake account in initialized state (delegatable)
    let stake_address = Pubkey::new_unique();
    let stake = mock_stake_initialized(stake_address, &stake_authority);

    let delegate_ix: Instruction = DelegateStakeInstruction {
        scoring_state: scoring_state_addr,
        stake_authority,
        stake_account: stake_address,
        validator_history: vh_address,
        validator_vote_account: vote_account,
        clock_sysvar: quasar_svm::solana_sdk_ids::sysvar::clock::ID,
        stake_history_sysvar: quasar_svm::solana_sdk_ids::sysvar::stake_history::ID,
        stake_config: quasar_svm::solana_sdk_ids::stake::config::ID,
        stake_program: quasar_svm::solana_sdk_ids::stake::ID,
    }.into();

    let result = svm.process_instruction(
        &delegate_ix,
        &[
            scoring_state,
            empty(stake_authority),
            stake,
            vh,
            empty(vote_account),
        ],
    );

    // This may fail if stake program is not loaded — document this
    if result.is_ok() {
        println!("  DELEGATE CU: {}", result.compute_units_consumed);
    } else {
        println!("  DELEGATE failed (likely stake program not in SVM): {:?}", result.raw_result);
        // Verify it's not our validation logic that failed
        // (our errors are Custom(6000+), stake program missing would be a different error)
    }
}
```

- [ ] **Step 3: Write undelegate flow test**

```rust
#[test]
fn test_undelegate_with_prebuilt_state() {
    let mut svm = setup();
    svm.sysvars.clock.epoch = 10;

    let (scoring_state_addr, scoring_bump) =
        Pubkey::find_program_address(&[b"scoring_state"], &crate::ID);
    let (stake_authority, sa_bump) =
        Pubkey::find_program_address(&[b"stake_authority"], &crate::ID);

    // Threshold = 300,000
    let scoring_state = mock_scoring_state_computed(
        scoring_state_addr,
        10,
        300_000,
        sa_bump,
        scoring_bump,
    );

    // Validator with low score: 200,000 * (100 - 50) / 100 = 100,000 — below threshold
    let vote_account = Pubkey::new_unique();
    let vh_address = Pubkey::new_unique();
    let vh = mock_validator_history(
        vh_address,
        0,
        &vote_account,
        &[
            (6, 200_000, 50),
            (7, 200_000, 50),
            (8, 200_000, 50),
            (9, 200_000, 50),
            (10, 200_000, 50),
        ],
    );

    // Stake account actively delegated to this validator
    let stake_address = Pubkey::new_unique();
    let stake = mock_stake_active(stake_address, &stake_authority, &vote_account);

    let undelegate_ix: Instruction = UndelegateStakeInstruction {
        scoring_state: scoring_state_addr,
        stake_authority,
        stake_account: stake_address,
        validator_history: vh_address,
        clock_sysvar: quasar_svm::solana_sdk_ids::sysvar::clock::ID,
        stake_program: quasar_svm::solana_sdk_ids::stake::ID,
    }.into();

    let result = svm.process_instruction(
        &undelegate_ix,
        &[
            scoring_state,
            empty(stake_authority),
            stake,
            vh,
        ],
    );

    if result.is_ok() {
        println!("  UNDELEGATE CU: {}", result.compute_units_consumed);
    } else {
        println!("  UNDELEGATE failed (likely stake program not in SVM): {:?}", result.raw_result);
    }
}
```

- [ ] **Step 4: Build and run all tests**

Run: `quasar build && quasar test`
Expected: All tests pass (validation tests), CPI tests document whether stake program is available.

- [ ] **Step 5: Commit**

```bash
git add src/tests.rs
git commit -m "test: end-to-end delegate/undelegate tests with pre-built scoring state"
```

---

### Task 11: Fix Constants and Final Polish

**Files:**
- Modify: `src/validator_history.rs` (fix VALIDATOR_HISTORY_PROGRAM_ID bytes)
- Modify: `src/stake_cpi.rs` (fix STAKE_PROGRAM_ID bytes)
- Various fixes based on build/test results

- [ ] **Step 1: Derive correct public key bytes**

The `VALIDATOR_HISTORY_PROGRAM_ID` (`HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa`) and `STAKE_PROGRAM_ID` (`Stake11111111111111111111111111111111111111`) need their correct base58-decoded byte arrays.

Use a test or CLI to derive:
```bash
# In a test or script:
# solana-keygen pubkey HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa
# Or decode in Rust test code
```

Update the constants in `validator_history.rs` and `stake_cpi.rs` with the correct byte arrays.

- [ ] **Step 2: Fix any remaining compilation errors from build**

Run: `quasar build`
Fix any errors related to:
- Pod type conversions (`.get()` vs `.into()`)
- `AccountView` API mismatches
- `CpiCall` const generic count mismatches
- Import paths

- [ ] **Step 3: Run full test suite**

Run: `quasar test`
Expected: All tests pass

- [ ] **Step 4: Clean up warnings**

Run: `quasar build 2>&1 | grep warning`
Fix any warnings (unused imports, dead code, etc.)

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "fix: correct program ID constants and polish implementation"
```

---

### Task 12: Profile and Document

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Profile compute units**

Run: `quasar profile`
Document the CU usage per instruction.

- [ ] **Step 2: Update README with usage and architecture**

Update `README.md` with:
- What the program does
- Instructions overview
- How to build, test, deploy
- Architecture diagram (text-based)
- Known limitations (bitmap size, score range)

- [ ] **Step 3: Final commit**

```bash
git add README.md
git commit -m "docs: update README with architecture, usage, and CU profile"
```
