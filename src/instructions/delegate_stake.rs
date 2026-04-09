use quasar_lang::prelude::*;
use quasar_lang::{cpi::Seed, sysvars::Sysvar as _};

use crate::{
    errors::QuanductorError,
    stake_cpi::{self, StakeProgram},
    stake_state,
    state::{ScoringState, EPOCHS_LOOKBACK, PHASE_THRESHOLD_COMPUTED},
    validator_history::{self, VH_PROGRAM_ID},
};

#[derive(Accounts)]
pub struct DelegateStake<'info> {
    #[account(seeds = ScoringState::seeds(), bump = scoring_state.bump)]
    pub scoring_state: &'info Account<ScoringState>,
    #[account(mut)]
    pub stake_account: &'info mut UncheckedAccount,
    pub validator_history: &'info UncheckedAccount,
    pub validator_vote_account: &'info UncheckedAccount,
    pub clock_sysvar: &'info UncheckedAccount,
    pub stake_history_sysvar: &'info UncheckedAccount,
    pub stake_config: &'info UncheckedAccount,
    pub stake_authority: &'info UncheckedAccount,
    pub stake_program: &'info Program<StakeProgram>,
}

impl<'info> DelegateStake<'info> {
    #[inline(always)]
    pub fn handler(&self) -> Result<(), ProgramError> {
        // Assert phase == THRESHOLD_COMPUTED
        if self.scoring_state.phase != PHASE_THRESHOLD_COMPUTED {
            return Err(QuanductorError::InvalidPhase.into());
        }

        // Check epoch freshness
        let clock = Clock::get()?;
        let current_epoch = clock.epoch.get();
        if self.scoring_state.epoch.get() != current_epoch {
            return Err(QuanductorError::EpochMismatch.into());
        }

        // Validate ValidatorHistory account
        let vh_view = self.validator_history.to_account_view();
        let vh_data = unsafe { vh_view.borrow_unchecked() };
        let vh_owner = vh_view.owner();
        validator_history::validate_validator_history(vh_data, vh_owner, &VH_PROGRAM_ID)?;

        // Compute validator's score and assert >= threshold
        let score = validator_history::compute_score(vh_data, current_epoch, EPOCHS_LOOKBACK);
        let threshold = self.scoring_state.threshold.get();
        if score < threshold {
            return Err(QuanductorError::ScoreBelowThreshold.into());
        }

        // Validate stake account is delegatable
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
        let sa_bump: u8 = self.scoring_state.stake_authority_bump;
        let seeds = [
            Seed::from(b"stake_authority" as &[u8]),
            Seed::from(core::slice::from_ref(&sa_bump)),
        ];

        stake_cpi::delegate_stake_cpi(
            self.stake_program.to_account_view(),
            self.stake_account.to_account_view(),
            self.validator_vote_account.to_account_view(),
            self.clock_sysvar.to_account_view(),
            self.stake_history_sysvar.to_account_view(),
            self.stake_config.to_account_view(),
            self.stake_authority.to_account_view(),
            &seeds,
        )
    }
}
