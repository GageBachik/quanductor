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
        self.scoring_state.bump = bumps.scoring_state;

        // Derive the stake_authority PDA bump on-chain (~544 CU, only runs once)
        let (_, sa_bump) = quasar_lang::pda::based_try_find_program_address(
            &[b"stake_authority"],
            &crate::ID,
        )?;
        self.scoring_state.stake_authority_bump = sa_bump;

        Ok(())
    }
}
