use quasar_lang::prelude::*;

use crate::state::ScoringState;

#[derive(Accounts)]
pub struct Initialize<'info> {
    pub payer: &'info mut Signer,
    #[account(init, payer = payer, seeds = ScoringState::seeds(), bump)]
    pub scoring_state: &'info mut Account<ScoringState>,
    pub system_program: &'info Program<System>,
}

impl<'info> Initialize<'info> {
    #[inline(always)]
    pub fn handler(&mut self, bumps: &InitializeBumps) -> Result<(), ProgramError> {
        // Account data is zeroed on init, so phase=0 (IDLE), epoch=0, threshold=0,
        // total_scored=0, histogram=all zeros, bitmap=all zeros are already correct.
        // We only need to set the bump fields.
        self.scoring_state.stake_authority_bump = bumps.scoring_state; // temporary — will derive SA bump later
        self.scoring_state.bump = bumps.scoring_state;
        Ok(())
    }
}
