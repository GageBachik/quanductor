# Quanductor — Validator Stake Delegation Program

## Overview

Quanductor is a Solana on-chain program built with the Quasar framework that scores all validators using Stakenet's Validator History data and manages native stake delegation to top-performing validators. It computes a staking rewards proxy (epoch credits adjusted by commission) over the last 5 epochs, determines the 90th percentile threshold via a histogram, and delegates/undelegates stake accounts accordingly. All instructions are permissionless.

## Architecture

```
Phase 1: Crank Histogram     Phase 2: Compute Threshold     Phase 3: Delegate/Undelegate
─────────────────────────     ──────────────────────────     ──────────────────────────────
Keeper sends ~28-54 txs      Single tx walks histogram      Per-stake-account txs
Each tx scores a batch of    from top to find 90th          Recompute single validator
validators, increments       percentile bucket boundary,    score, compare to stored
histogram buckets             stores threshold               threshold, CPI stake program
```

### Framework

- **Quasar** (`quasar-lang`) — zero-copy, zero-allocation, `#![no_std]` Solana framework
- **Not Anchor** — ValidatorHistory is an Anchor program; we read its accounts via manual zero-copy deserialization

### Approach

- **Approach A (selected):** Single `ScoringState` PDA, inline `#[repr(C)]` structs for ValidatorHistory layout, raw CPI for stake program
- Rejected: Split state (over-engineered), `declare_program!` for ValidatorHistory (unproven with Anchor IDLs in beta Quasar)

## Score Formula

```
For each of the last 5 epochs with valid data in ValidatorHistory:
  reward_i = epoch_credits_i * (100 - commission_i) / 100

score = sum(reward_i) / num_valid_epochs
```

- `epoch_credits`: u32 from `ValidatorHistoryEntry` (offset 12)
- `commission`: u8 from `ValidatorHistoryEntry` (offset 16), range 0-100
- If fewer than 1 valid epoch exists, score = 0
- Entries with `epoch == u16::MAX` or `commission == u8::MAX` are unset — skip

## Histogram Design

| Parameter | Value |
|-----------|-------|
| Buckets | 512 |
| Score range | 0 – 420,000 |
| Bucket width | ~820 score points |
| Precision | ~0.2% of max score |
| Storage | 512 x 2 bytes = 1,024 bytes |

Bucket index: `min(score * 512 / 420_001, 511)`

Scores above 420,000 clamp to bucket 511. The range covers realistic epoch credits with headroom.

### 90th Percentile Computation

Walk histogram from bucket 511 down to 0, accumulating counts. When `running_count >= total_scored / 10`, the current bucket's lower bound is the threshold:

```
threshold = bucket * 420_001 / 512
```

## On-Chain Accounts

### ScoringState PDA

Seeds: `[b"scoring_state"]`

```rust
#[account(discriminator = 1)]
pub struct ScoringState {
    pub phase: u8,               // 0=Idle, 1=Cranking, 2=ThresholdComputed
    pub epoch: u64,              // scoring round epoch
    pub threshold: u64,          // 90th percentile score
    pub total_scored: u16,       // validators scored this round
    pub histogram: [u16; 512],   // bucket counts (1,024 bytes)
    pub bitmap: [u8; 768],       // dedup bitmap (6,144 validators max)
    pub stake_authority_bump: u8,// StakeAuthority PDA bump (for invoke_signed)
    pub bump: u8,                // PDA bump — always last
}
```

- Size: ~1,812 bytes
- Rent: ~0.014 SOL
- Bitmap supports up to 6,144 validators (current set ~1,700, Stakenet max 5,000)

### StakeAuthority PDA

Seeds: `[b"stake_authority"]`

No stored data. Used only as a PDA signer for stake program CPIs (delegate, deactivate). Passed as `UncheckedAccount`.

## ValidatorHistory (Foreign Account — Read Only)

We define our own `#[repr(C)]` structs matching Stakenet's byte layout:

### ValidatorHistoryEntry (128 bytes)

```rust
#[repr(C)]
pub struct ValidatorHistoryEntry {
    pub activated_stake_lamports: [u8; 8],  // offset 0
    pub epoch: [u8; 2],                      // offset 8
    pub mev_commission: [u8; 2],             // offset 10
    pub epoch_credits: [u8; 4],              // offset 12  ← used
    pub commission: u8,                       // offset 16  ← used
    pub _rest: [u8; 111],                    // offset 17, pad to 128
}
```

### CircBuf (65,560 bytes)

```rust
#[repr(C)]
pub struct CircBuf {
    pub idx: [u8; 8],                          // offset 0
    pub is_empty: u8,                          // offset 8
    pub _padding: [u8; 7],                     // offset 9
    pub arr: [ValidatorHistoryEntry; 512],     // offset 16
}
```

### ValidatorHistory (65,856 bytes, plus 8-byte Anchor discriminator)

Key offsets from start of account data (including 8-byte Anchor discriminator):
- `index` (u32): offset 44 (8 + 36)
- `history.idx` (u64): offset 304 (8 + 296)
- `history.is_empty` (u8): offset 312 (8 + 304)
- `history.arr[0]`: offset 320 (8 + 312)

**Validation:**
- Owner == `HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa`
- First 8 bytes == `[205, 25, 8, 221, 253, 131, 2, 146]`
- `data.len() >= 65,864`

**Program ID:** `HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa`

## Instructions

### 1. `initialize` (discriminator = 0)

Creates the `ScoringState` PDA. Called once. Permissionless.

**Accounts:**
- `payer`: `&'info mut Signer` — pays rent
- `scoring_state`: `&'info mut Account<ScoringState>` — `#[account(init, seeds = [b"scoring_state"], bump, payer = payer)]`
- `system_program`: `&'info Program<System>`

**Logic:**
1. Initialize ScoringState with phase=Idle, epoch=0, threshold=0, zeroed histogram/bitmap

### 2. `crank_scores` (discriminator = 1)

Batch-scores validators from remaining accounts. Permissionless — anyone can crank.

**Accounts:**
- `scoring_state`: `&'info mut Account<ScoringState>` — `#[account(mut, seeds = [b"scoring_state"], bump = scoring_state.bump)]`
- `clock`: `&'info Sysvar<Clock>` (or via syscall)
- Remaining accounts: `ValidatorHistory` accounts (batch of ~32 per tx)

Uses `CtxWithRemaining` to accept variable-length ValidatorHistory batches.

**Logic:**
1. Get current epoch from Clock
2. If `current_epoch != scoring_state.epoch` → reset histogram, bitmap, total_scored; set epoch, phase=Cranking
3. Assert phase == Cranking
4. For each ValidatorHistory in remaining accounts:
   a. Validate owner + discriminator + data length
   b. Read `index` (u32) → check bitmap; if already scored, skip
   c. Read `history.idx` → locate circular buffer position
   d. Walk backwards from idx, find entries matching current_epoch-0 through current_epoch-4
   e. For each valid entry: `reward = epoch_credits * (100 - commission) / 100`
   f. `score = sum / num_valid_epochs` (0 if no valid epochs)
   g. `bucket = min(score * 512 / 420_001, 511)`
   h. Increment `histogram[bucket]`
   i. Set bitmap bit for this validator
   j. Increment `total_scored`

**CU estimate:** ~2,950 CU per validator x 32 = ~94,400 CU per tx

### 3. `compute_threshold` (discriminator = 2)

Single tx. Walks histogram top-down to find 90th percentile. Permissionless.

**Accounts:**
- `scoring_state`: `&'info mut Account<ScoringState>` — `#[account(mut, seeds = [b"scoring_state"], bump = scoring_state.bump)]`

**Logic:**
1. Assert phase == Cranking
2. Assert `total_scored >= 1400` (minimum validator coverage)
3. `target_rank = total_scored / 10` (top 10%)
4. Walk buckets 511 → 0:
   - `running_count += histogram[bucket]`
   - If `running_count >= target_rank`: `threshold = bucket * 420_001 / 512`; break
5. Store threshold in scoring_state
6. Set phase = ThresholdComputed

**CU estimate:** ~10,000 CU

### 4. `delegate_stake` (discriminator = 3)

Delegates an inactive/deactivated stake account to a qualifying validator. Permissionless.

**Accounts:**
- `scoring_state`: `&'info Account<ScoringState>` — `#[account(seeds = [b"scoring_state"], bump = scoring_state.bump)]`
- `stake_authority`: `&'info UncheckedAccount` — `#[account(seeds = [b"stake_authority"], bump)]`
- `stake_account`: `&'info mut UncheckedAccount`
- `validator_history`: `&'info UncheckedAccount` — target validator's history
- `validator_vote_account`: `&'info UncheckedAccount`
- `clock`: `&'info UncheckedAccount` (sysvar)
- `stake_history`: `&'info UncheckedAccount` (sysvar)
- `stake_config`: `&'info UncheckedAccount` (sysvar)
- `stake_program`: `&'info UncheckedAccount`

**Logic:**
1. Assert phase == ThresholdComputed
2. Assert `scoring_state.epoch == current_epoch` (threshold is fresh)
3. Read validator's last 5 epochs from validator_history, compute score
4. Assert score >= threshold
5. Parse stake account data: assert staker authority == StakeAuthority PDA
6. Assert stake account is in delegatable state (inactive or fully deactivated)
7. Verify validator_history.vote_account matches validator_vote_account
8. CPI → `StakeInstruction::DelegateStake` signed by StakeAuthority PDA

**CU estimate:** ~8,050 CU

### 5. `undelegate_stake` (discriminator = 4)

Deactivates stake from a validator below the threshold. Permissionless.

**Accounts:**
- `scoring_state`: `&'info Account<ScoringState>` — `#[account(seeds = [b"scoring_state"], bump = scoring_state.bump)]`
- `stake_authority`: `&'info UncheckedAccount` — `#[account(seeds = [b"stake_authority"], bump)]`
- `stake_account`: `&'info mut UncheckedAccount`
- `validator_history`: `&'info UncheckedAccount` — currently delegated validator's history
- `clock`: `&'info UncheckedAccount` (sysvar)
- `stake_program`: `&'info UncheckedAccount`

**Logic:**
1. Assert phase == ThresholdComputed
2. Assert `scoring_state.epoch == current_epoch`
3. Read validator's last 5 epochs from validator_history, compute score
4. Assert score < threshold
5. Parse stake account data: assert staker authority == StakeAuthority PDA
6. Assert stake account is currently delegated to this validator (match vote pubkey)
7. CPI → `StakeInstruction::Deactivate` signed by StakeAuthority PDA

**CU estimate:** ~6,000 CU

## Stake Program CPI

Built manually via Quasar's `CpiCall`:

### DelegateStake (instruction index 2)

- Instruction data: `[2, 0, 0, 0]` (LE u32)
- Accounts: `[stake(w,s), vote, clock, stake_history, stake_config, stake_authority(s)]`
- Signed by StakeAuthority PDA via `invoke_signed(&[&[b"stake_authority", &[bump]]])`

### Deactivate (instruction index 5)

- Instruction data: `[5, 0, 0, 0]` (LE u32)
- Accounts: `[stake(w), clock, stake_authority(s)]`
- Signed by StakeAuthority PDA via `invoke_signed(&[&[b"stake_authority", &[bump]]])`

## Stake Account State Parsing

Stake accounts use a known layout (200 bytes). Key fields:
- Offset 0-3: stake state enum (u32 LE) — 0=Uninitialized, 1=Initialized, 2=Stake, 3=RewardsPool
- Offset 4-67: Meta (rent_exempt_reserve, authorized staker/withdrawer)
  - Staker pubkey at offset 12 (32 bytes)
- Offset 68+: Stake data (delegation voter, stake, activation/deactivation epoch)
  - Voter pubkey at offset 124 (32 bytes)
  - Deactivation epoch at offset 172 (u64 LE)

**Delegatable state:** state == 1 (Initialized, never delegated) OR state == 2 AND deactivation_epoch < current_epoch (fully cooled down).

**Active/delegated state:** state == 2 AND deactivation_epoch == u64::MAX.

## Epoch Transition Handling

The `crank_scores` instruction handles epoch changes automatically:
- If `current_epoch != scoring_state.epoch`: reset histogram, bitmap, total_scored; set new epoch; set phase=Cranking
- This works whether the previous round was complete or partial
- No manual intervention needed — system is self-healing

## Duplication Protection

Bitmap indexed by validator's `index` field from ValidatorHistory (u32, stable per-validator):
- Check: `bitmap[index / 8] & (1 << (index % 8)) != 0`
- Set: `bitmap[index / 8] |= 1 << (index % 8)`
- Reset: zero entire 768-byte bitmap on epoch change
- Max validators: 6,144 (768 bytes x 8 bits)

If a validator is already scored, skip silently (no error — allows idempotent cranking).

## File Structure

```
src/
  lib.rs                       — declare_id!, #[program] with 5 instructions
  state.rs                     — ScoringState, ScoringPhase constants
  errors.rs                    — #[error_code] enum
  instructions/
    mod.rs                     — module exports
    initialize.rs              — Initialize accounts struct + handler
    crank_scores.rs            — CrankScores accounts struct + handler
    compute_threshold.rs       — ComputeThreshold accounts struct + handler
    delegate_stake.rs          — DelegateStake accounts struct + handler
    undelegate_stake.rs        — UndelegateStake accounts struct + handler
  validator_history.rs         — #[repr(C)] structs + reading/validation helpers
  stake_cpi.rs                 — Stake program CPI builders (delegate, deactivate)
  stake_state.rs               — Stake account parsing helpers
```

## Error Codes

Starting at 6000 (Quasar convention):

| Code | Name | When |
|------|------|------|
| 6000 | `InvalidPhase` | Wrong phase for this instruction |
| 6001 | `EpochMismatch` | Threshold is stale (wrong epoch) |
| 6002 | `AlreadyScored` | Validator already in bitmap (unused — we skip silently) |
| 6003 | `InsufficientValidators` | total_scored < 1,400 for threshold |
| 6004 | `ScoreBelowThreshold` | Validator score too low for delegation |
| 6005 | `ScoreAboveThreshold` | Validator score too high for undelegation |
| 6006 | `InvalidStakeState` | Stake account not in expected state |
| 6007 | `InvalidStakeAuthority` | Staker authority != StakeAuthority PDA |
| 6008 | `InvalidValidatorHistory` | Bad owner, discriminator, or data length |
| 6009 | `InsufficientEpochData` | Validator has no valid epoch data |
| 6010 | `InvalidVoteAccount` | Vote account mismatch between stake and history |

## Security Considerations

### Permissionless Safety
- All inputs validated against on-chain state
- Score computation is deterministic — same inputs, same output
- Bitmap prevents double-counting / histogram inflation
- Epoch guard prevents stale data usage
- Threshold derived entirely from on-chain data

### Edge Cases
- **Bucket boundaries:** Validators at exact bucket boundary may be misclassified by ~820 points (~0.2% at typical scores). Acceptable.
- **Score range overflow:** Epoch credits > 420,000 clamp to bucket 511. Monitor and upgrade if needed.
- **Partial cranking:** Minimum 1,400 validators required before threshold computation.
- **Missing history:** Validators with < 1 valid epoch get score 0 — won't be delegated to.
- **Selective cranking attack:** Mitigated by minimum validator count requirement.

### Known Limitations
- Bitmap supports max 6,144 validators. If Solana exceeds this, program upgrade required.
- Score range hardcoded to 420,000. If epoch credits increase dramatically, needs upgrade.
- No admin/pause mechanism (pure permissionless as spec requires).

## Cost Summary

| Item | Cost |
|------|------|
| ScoringState rent | ~0.014 SOL |
| Crank txs per epoch | ~28-54 txs x 5,000 lamports = ~0.00027 SOL |
| Compute threshold tx | 1 tx x 5,000 lamports |
| Delegate/undelegate txs | per stake account x 5,000 lamports |
| **Total per epoch** | **< 0.02 SOL** |

## Testing Plan

All tests use QuasarSVM (Rust) — Quasar's built-in test harness with `quasar-svm` crate.

### Unit Tests

#### Score Computation (`test_score_computation`)
- Validator with 5 full epochs of data → correct average
- Validator with 3 valid epochs, 2 unset → averages only valid epochs
- Validator with 0 valid epochs → score = 0
- Validator with 0% commission → score = raw epoch_credits average
- Validator with 100% commission → score = 0
- Validator with varying commission across epochs → weighted correctly
- Maximum epoch_credits (420,000+) → clamps to bucket 511
- Edge: epoch_credits = 0 → score = 0 regardless of commission

#### Histogram Bucketing (`test_histogram_bucketing`)
- Score 0 → bucket 0
- Score 420,000 → bucket 511
- Score at exact bucket boundary → correct bucket
- Score above 420,000 → bucket 511 (clamped)
- Uniform distribution → roughly equal bucket counts

#### Bitmap Operations (`test_bitmap`)
- Set bit at index 0, 1, 7, 8, 6143 → check correctly
- Double-set same index → idempotent
- Check unset bit → returns false
- Reset bitmap → all bits cleared
- Index at max (6,143) → works
- Index beyond max (6,144+) → handled safely

#### Threshold Computation (`test_threshold_computation`)
- 10 validators, top 1 should be above threshold
- 100 validators uniformly distributed → threshold at ~90th bucket
- All validators in same bucket → threshold = that bucket's lower bound
- All validators in top bucket → threshold = top bucket's lower bound
- 1,700 validators with realistic score distribution → reasonable threshold

#### Circular Buffer Walking (`test_circ_buf_reading`)
- Buffer idx at 0 → reads entries wrapping around from 511
- Buffer idx at 255 → reads 5 entries backwards correctly
- Buffer with is_empty = 1 → no valid entries
- Entries for wrong epochs → skipped correctly
- Entry at wrap-around boundary → reads across correctly

### Integration Tests

#### Initialize (`test_initialize`)
- Happy path: creates ScoringState PDA with correct initial values
- Double-init: fails (account already exists)
- Verify bump stored correctly (last field)
- Verify phase = Idle, epoch = 0, zeroed histogram/bitmap

#### Crank Scores (`test_crank_scores`)
- Happy path: score 1 validator → histogram updated, bitmap set, total_scored = 1
- Batch of 32 validators → all scored correctly
- Duplicate validator in same batch → second one skipped
- Duplicate validator across two txs → second one skipped
- Epoch transition mid-cranking → resets state, starts fresh round
- Invalid ValidatorHistory owner → rejected
- Invalid ValidatorHistory discriminator → rejected
- ValidatorHistory with too-short data → rejected
- Phase = ThresholdComputed → epoch change resets, allows cranking

#### Compute Threshold (`test_compute_threshold`)
- Happy path: after scoring 1,700 validators → threshold computed, phase = ThresholdComputed
- Insufficient validators (< 1,400) → fails with InsufficientValidators
- Wrong phase (Idle) → fails with InvalidPhase
- Wrong phase (ThresholdComputed) → fails with InvalidPhase
- Threshold value is mathematically correct for known score distribution
- Double-call → fails (already ThresholdComputed)

#### Delegate Stake (`test_delegate_stake`)
- Happy path: inactive stake account + qualifying validator → delegated
- Happy path: fully deactivated stake account → re-delegated
- Validator below threshold → fails with ScoreBelowThreshold
- Stake account already active → fails with InvalidStakeState
- Wrong staker authority → fails with InvalidStakeAuthority
- Stale threshold (wrong epoch) → fails with EpochMismatch
- Wrong phase (Cranking) → fails with InvalidPhase
- Vote account mismatch → fails with InvalidVoteAccount

#### Undelegate Stake (`test_undelegate_stake`)
- Happy path: active stake + validator below threshold → deactivated
- Validator above threshold → fails with ScoreAboveThreshold
- Stake account not active → fails with InvalidStakeState
- Wrong staker authority → fails with InvalidStakeAuthority
- Stale threshold → fails with EpochMismatch
- Wrong phase → fails with InvalidPhase
- Stake delegated to different validator than history provided → fails

### End-to-End Tests

#### Full Epoch Cycle (`test_full_epoch_cycle`)
1. Initialize
2. Crank all validators across multiple txs
3. Compute threshold
4. Delegate an inactive stake account to top validator
5. Undelegate from a bottom validator
6. Verify stake account states changed correctly

#### Epoch Transition (`test_epoch_transition`)
1. Complete a full scoring cycle in epoch N
2. Advance to epoch N+1
3. Crank scores for new epoch (auto-resets)
4. Verify old histogram/bitmap cleared
5. Complete new cycle, verify new threshold

#### Re-delegation Flow (`test_redelegation_flow`)
1. Stake account delegated to validator X (above threshold)
2. Next epoch: validator X drops below threshold
3. Undelegate stake from X (deactivate)
4. Wait for cooldown (advance epoch)
5. Delegate to validator Y (above threshold)
6. Verify complete flow works

### Test Helpers

- `create_mock_validator_history(index, epoch_credits, commission)` — builds a fake ValidatorHistory account with specified data
- `create_mock_stake_account(state, staker, voter)` — builds a fake stake account in specified state
- `setup_scoring_state(phase, epoch, threshold, scored_validators)` — creates pre-configured ScoringState
- `advance_epoch(svm)` — advances the clock to next epoch
