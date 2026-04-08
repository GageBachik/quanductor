<h1 align="center">
  <code>quanductor</code>
</h1>
<p align="center">
  <img width="400" alt="Quanductor" src="https://i.postimg.cc/fL8Hnffn/Conductor-in-cartoon-emblem.png" />
</p>
<p align="center">
  Staked SOL managed and optimized automatically.
</p>

## Overview

Quanductor is a Solana on-chain program that scores all validators using [Stakenet's Validator History](https://github.com/jito-foundation/stakenet) data and manages native stake delegation to top-performing validators. Built with the [Quasar](https://github.com/blueshift-gg/quasar) framework.

### How It Works

1. A permissionless keeper cranks validator scores each epoch
2. Scores are computed as `avg(epoch_credits * (100 - commission) / 100)` over the last 5 epochs
3. A 512-bucket histogram determines the 90th percentile threshold
4. Stake accounts are delegated to validators above the threshold
5. Stake accounts are undelegated from validators below the threshold

### Architecture

```
Phase 1: Crank Histogram     Phase 2: Compute Threshold     Phase 3: Delegate/Undelegate
─────────────────────────     ──────────────────────────     ──────────────────────────────
Keeper sends ~28-54 txs      Single tx walks histogram      Per-stake-account txs
Each tx scores a batch of    from top to find 90th          Recompute single validator
validators, increments       percentile bucket boundary,    score, compare to stored
histogram buckets             stores threshold               threshold, CPI stake program
```

## Instructions

| # | Instruction | Description |
|---|-------------|-------------|
| 0 | `initialize` | Create the ScoringState PDA |
| 1 | `crank_scores` | Batch-score validators from remaining accounts |
| 2 | `compute_threshold` | Walk histogram to find 90th percentile |
| 3 | `delegate_stake` | Delegate inactive stake to qualifying validator |
| 4 | `undelegate_stake` | Deactivate stake from underperforming validator |

All instructions are permissionless.

## State

**ScoringState PDA** (`seeds = [b"scoring_state"]`):
- 512-bucket histogram (1,024 bytes) for score distribution
- 768-byte bitmap (6,144 validators max) for dedup
- Phase tracking: Idle -> Cranking -> ThresholdComputed
- Auto-resets on epoch change

## Build

```bash
quasar build
```

## Test

```bash
quasar test
```

## Known Limitations

- Bitmap supports max 6,144 validators (current set ~1,700)
- Score range hardcoded to 420,000 (covers realistic epoch credits)
- No admin/pause mechanism (pure permissionless)
- Histogram bucket boundary precision: ~0.2% at typical scores

## Dependencies

- [Quasar](https://github.com/blueshift-gg/quasar) — Zero-copy Solana program framework
- [Stakenet Validator History](https://github.com/jito-foundation/stakenet) — On-chain validator metrics (read-only)
- Solana Native Stake Program — Delegate/deactivate CPI
