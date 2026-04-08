#![cfg_attr(not(test), no_std)]

use quasar_lang::prelude::*;

mod errors;
mod instructions;
mod state;
mod validator_history;
mod stake_cpi;
mod stake_state;
use instructions::*;

declare_id!("4qoALqJXrrjcqTmetedH55rvHTeF4XPfFVo8GaztD6KR");

#[program]
mod quanductor {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {
        ctx.accounts.handler(&ctx.bumps)
    }

    #[instruction(discriminator = 1)]
    pub fn crank_scores(ctx: CtxWithRemaining<CrankScores>) -> Result<(), ProgramError> {
        ctx.accounts.handler(ctx.remaining_accounts())
    }

    #[instruction(discriminator = 2)]
    pub fn compute_threshold(ctx: Ctx<ComputeThreshold>) -> Result<(), ProgramError> {
        ctx.accounts.handler()
    }

    #[instruction(discriminator = 3)]
    pub fn delegate_stake(ctx: Ctx<DelegateStake>) -> Result<(), ProgramError> {
        ctx.accounts.handler()
    }

    #[instruction(discriminator = 4)]
    pub fn undelegate_stake(ctx: Ctx<UndelegateStake>) -> Result<(), ProgramError> {
        ctx.accounts.handler()
    }
}

#[cfg(test)]
mod tests;
