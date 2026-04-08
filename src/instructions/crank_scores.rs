use quasar_lang::{prelude::*, remaining::RemainingAccounts, sysvars::Sysvar as _};

use crate::{
    errors::QuanductorError,
    state::{ScoringState, EPOCHS_LOOKBACK, HISTOGRAM_BYTES, PHASE_CRANKING},
    validator_history::{
        bitmap_is_set, bitmap_set, compute_score, read_vh_index, score_to_bucket,
        validate_validator_history, VH_PROGRAM_ID,
    },
};

#[derive(Accounts)]
pub struct CrankScores<'info> {
    pub payer: &'info Signer,
    #[account(mut, seeds = ScoringState::seeds(), bump = scoring_state.bump)]
    pub scoring_state: &'info mut Account<ScoringState>,
}

impl<'info> CrankScores<'info> {
    #[inline(always)]
    pub fn handler(&mut self, remaining: RemainingAccounts) -> Result<(), ProgramError> {
        let clock = Clock::get()?;
        let current_epoch = clock.epoch.get();

        // If epoch changed, reset state for the new epoch
        if current_epoch != self.scoring_state.epoch.get() {
            // Zero out histogram
            let mut i = 0;
            while i < HISTOGRAM_BYTES {
                self.scoring_state.histogram[i] = 0;
                i += 1;
            }
            // Zero out bitmap
            i = 0;
            while i < self.scoring_state.bitmap.len() {
                self.scoring_state.bitmap[i] = 0;
                i += 1;
            }
            self.scoring_state.epoch = current_epoch.into();
            self.scoring_state.phase = PHASE_CRANKING;
            self.scoring_state.total_scored = 0u16.into();
            self.scoring_state.threshold = 0u64.into();
        }

        // Assert phase is CRANKING
        if self.scoring_state.phase != PHASE_CRANKING {
            return Err(QuanductorError::InvalidPhase.into());
        }

        // Process each ValidatorHistory account from remaining accounts
        for account in remaining.iter() {
            let account = account?;

            // SAFETY: remaining accounts iterator guarantees no duplicates (strict mode),
            // and we only read the data without holding other borrows.
            let data = unsafe { account.borrow_unchecked() };
            let owner = account.owner();

            // Validate this is a legitimate ValidatorHistory account
            validate_validator_history(data, owner, &VH_PROGRAM_ID)?;

            // Read the validator index
            let index = read_vh_index(data);

            // Skip if already scored in this epoch
            if bitmap_is_set(&self.scoring_state.bitmap, index) {
                continue;
            }

            // Compute the score
            let score = compute_score(data, current_epoch, EPOCHS_LOOKBACK);

            // Map score to histogram bucket
            let bucket = score_to_bucket(score);

            // Read current bucket count (u16 LE), increment, write back
            let offset = bucket * 2;
            let count =
                u16::from_le_bytes([
                    self.scoring_state.histogram[offset],
                    self.scoring_state.histogram[offset + 1],
                ]);
            let new_count = count.wrapping_add(1);
            let bytes = new_count.to_le_bytes();
            self.scoring_state.histogram[offset] = bytes[0];
            self.scoring_state.histogram[offset + 1] = bytes[1];

            // Mark validator as scored
            bitmap_set(&mut self.scoring_state.bitmap, index);

            // Increment total_scored
            let scored = self.scoring_state.total_scored.get();
            self.scoring_state.total_scored = scored.wrapping_add(1).into();
        }

        Ok(())
    }
}
