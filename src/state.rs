use quasar_lang::prelude::*;

pub const PHASE_IDLE: u8 = 0;
pub const PHASE_CRANKING: u8 = 1;
pub const PHASE_THRESHOLD_COMPUTED: u8 = 2;

pub const HISTOGRAM_BUCKETS: usize = 512;
pub const HISTOGRAM_BYTES: usize = HISTOGRAM_BUCKETS * 2; // 1024 bytes (512 x u16)
pub const BITMAP_BYTES: usize = 768;
pub const SCORE_RANGE: u64 = 420_001;
pub const MIN_VALIDATORS: u16 = 100;
pub const EPOCHS_LOOKBACK: usize = 5;

#[account(discriminator = 1)]
#[seeds(b"scoring_state")]
pub struct ScoringState {
    pub phase: u8,
    pub epoch: u64,
    pub threshold: u64,
    pub total_scored: u16,
    pub histogram: [u8; HISTOGRAM_BYTES],  // 512 x u16 LE, stored as raw bytes
    pub bitmap: [u8; BITMAP_BYTES],        // 6144-bit dedup bitmap
    pub stake_authority_bump: u8,
    pub bump: u8,
}
