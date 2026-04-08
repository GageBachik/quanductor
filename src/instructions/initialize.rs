use {
    crate::state::ScoringState,
    quasar_lang::prelude::*,
};

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
        let (_, stake_authority_bump) =
            quasar_lang::pda::based_try_find_program_address(
                &[b"stake_authority"],
                &crate::ID,
            )?;

        // Account is zeroed on init, so phase=0 (IDLE), epoch=0, threshold=0,
        // total_scored=0, histogram=all zeros, bitmap=all zeros are already set.
        // We only need to set the non-zero bump fields.
        self.scoring_state.stake_authority_bump = stake_authority_bump;
        self.scoring_state.bump = bumps.scoring_state;
        Ok(())
    }
}
