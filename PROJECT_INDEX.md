# Project Index: Quanductor

Generated: 2026-04-08

## Project Structure

```
quanductor/
├── Cargo.toml                    # Rust package — quasar-lang framework
├── Quasar.toml                   # Quasar config (solana toolchain, quasarsvm-rust tests)
├── README.md                     # Architecture, instructions, usage
├── src/
│   ├── lib.rs                    # Entry point — declare_id!, #[program] with 5 instructions
│   ├── state.rs                  # ScoringState PDA (histogram, bitmap, phase)
│   ├── errors.rs                 # QuanductorError enum (10 variants, codes 6000+)
│   ├── validator_history.rs      # Zero-copy reader for Stakenet ValidatorHistory accounts
│   ├── stake_cpi.rs              # CPI builders for native Stake program
│   ├── stake_state.rs            # Parse native stake account layout
│   ├── tests.rs                  # 12 tests using QuasarSVM
│   └── instructions/
│       ├── mod.rs                # Module exports
│       ├── initialize.rs         # [disc=0] Create ScoringState PDA
│       ├── crank_scores.rs       # [disc=1] Batch-score validators (CtxWithRemaining)
│       ├── compute_threshold.rs  # [disc=2] Walk histogram for 90th percentile
│       ├── delegate_stake.rs     # [disc=3] Delegate stake to qualifying validator
│       └── undelegate_stake.rs   # [disc=4] Deactivate from underperformer
├── docs/superpowers/
│   ├── specs/2026-04-08-quanductor-design.md
│   └── plans/2026-04-08-quanductor-implementation.md
└── target/
    ├── deploy/quanductor.so      # Compiled SBF binary (29.3 KB)
    ├── deploy/quanductor-keypair.json
    ├── idl/quanductor.idl.json
    └── client/rust/quanductor-client/  # Auto-generated instruction builders
```

## Entry Points

- **Program:** `src/lib.rs` — `declare_id!("4qoALqJXrrjcqTmetedH55rvHTeF4XPfFVo8GaztD6KR")`
- **Tests:** `src/tests.rs` — 12 tests via QuasarSVM
- **Client:** `target/client/rust/quanductor-client/src/lib.rs` — generated instruction types

## Core Modules

### state.rs (25 LOC)
- `ScoringState` — PDA account with `#[seeds(b"scoring_state")]`
- Fields: phase(u8), epoch(u64), threshold(u64), total_scored(u16), histogram([u8;1024]), bitmap([u8;768]), bumps
- Constants: PHASE_IDLE/CRANKING/THRESHOLD_COMPUTED, HISTOGRAM_BUCKETS=512, BITMAP_BYTES=768, SCORE_RANGE=420_001, MIN_VALIDATORS=1400, EPOCHS_LOOKBACK=5

### validator_history.rs (243 LOC)
- `VH_PROGRAM_ID` — HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa
- `validate_validator_history()` — owner + discriminator + length checks
- `compute_score()` — avg(credits * (100-commission)/100) over last 5 epochs
- `score_to_bucket()` — maps score to 0-511 histogram bucket
- `bitmap_is_set/set()` — dedup tracking

### stake_cpi.rs (77 LOC)
- `delegate_stake_cpi()` — CpiCall<6,4> to StakeInstruction::DelegateStake
- `deactivate_stake_cpi()` — CpiCall<3,4> to StakeInstruction::Deactivate
- `STAKE_PROGRAM_ID` — Stake11111111111111111111111111111111111111

### stake_state.rs (70 LOC)
- `read_staker/voter/deactivation_epoch()` — parse stake account offsets
- `is_delegatable()` — Initialized OR fully deactivated
- `is_active()` — Stake with deactivation_epoch == u64::MAX

## Instructions

| Disc | Name | Accounts Pattern | Key Logic |
|------|------|-----------------|-----------|
| 0 | initialize | Ctx<Initialize> | Creates ScoringState PDA |
| 1 | crank_scores | CtxWithRemaining<CrankScores> | Batch histogram + bitmap update |
| 2 | compute_threshold | Ctx<ComputeThreshold> | Walks histogram top-down |
| 3 | delegate_stake | Ctx<DelegateStake> | Score check + stake CPI |
| 4 | undelegate_stake | Ctx<UndelegateStake> | Score check + deactivate CPI |

## Configuration

- `Cargo.toml` — quasar-lang (git), solana-instruction 3.2.0, quasar-svm (dev)
- `Quasar.toml` — type=solana, testing=quasarsvm-rust

## Tests (12 passing)

| Test | Validates |
|------|-----------|
| test_initialize | PDA creation, discriminator, phase |
| test_initialize_double_init_fails | Idempotency guard |
| test_crank_single_validator | Scoring pipeline, histogram update |
| test_crank_duplicate_skipped | Bitmap dedup |
| test_crank_invalid_owner_rejected | Foreign account validation |
| test_epoch_transition_resets | Auto-reset on new epoch |
| test_compute_threshold_insufficient_validators | Min validator guard |
| test_compute_threshold_wrong_phase | Phase state machine |
| test_delegate_wrong_phase | Phase guard |
| test_delegate_below_threshold | Score threshold enforcement |
| test_undelegate_wrong_phase | Phase guard |
| test_undelegate_above_threshold | Score threshold enforcement |

## Quick Start

```bash
quasar build          # Compile to SBF (29.3 KB)
quasar test           # Run 12 tests (~7s)
quasar profile        # CU flamegraph (~3,013 CU baseline)
quasar deploy         # Deploy to cluster
```

## Stats

- **Total LOC:** 1,595 (source + tests)
- **Binary size:** 29.3 KB
- **Test count:** 12
- **Framework:** Quasar (zero-copy, #![no_std])
