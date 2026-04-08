use quasar_lang::prelude::*;

use crate::{
    errors::QuanductorError,
    state::{ScoringState, MIN_VALIDATORS, PHASE_CRANKING, PHASE_THRESHOLD_COMPUTED, SCORE_RANGE},
};

#[derive(Accounts)]
pub struct ComputeThreshold<'info> {
    #[account(mut, seeds = ScoringState::seeds(), bump = scoring_state.bump)]
    pub scoring_state: &'info mut Account<ScoringState>,
}

impl<'info> ComputeThreshold<'info> {
    #[inline(always)]
    pub fn handler(&mut self) -> Result<(), ProgramError> {
        // Assert phase == CRANKING
        if self.scoring_state.phase != PHASE_CRANKING {
            return Err(QuanductorError::InvalidPhase.into());
        }

        // Assert enough validators scored
        let total_scored = self.scoring_state.total_scored.get();
        if total_scored < MIN_VALIDATORS {
            return Err(QuanductorError::InsufficientValidators.into());
        }

        // Walk histogram from top (bucket 511) down to find 90th percentile
        let target_rank = total_scored as u64 / 10; // top 10%
        let mut running_count: u64 = 0;
        let mut threshold: u64 = 0;

        let mut bucket: usize = 512;
        while bucket > 0 {
            bucket -= 1;
            let offset = bucket * 2;
            let count = u16::from_le_bytes([
                self.scoring_state.histogram[offset],
                self.scoring_state.histogram[offset + 1],
            ]) as u64;
            running_count += count;
            if running_count >= target_rank {
                // Lower bound of this bucket
                threshold = (bucket as u64) * SCORE_RANGE / 512;
                break;
            }
        }

        self.scoring_state.threshold = threshold.into();
        self.scoring_state.phase = PHASE_THRESHOLD_COMPUTED;

        Ok(())
    }
}
