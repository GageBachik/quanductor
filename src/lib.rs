#![cfg_attr(not(test), no_std)]

use quasar_lang::prelude::*;

mod errors;
mod instructions;
mod state;
mod validator_history;
use instructions::*;

declare_id!("4qoALqJXrrjcqTmetedH55rvHTeF4XPfFVo8GaztD6KR");

#[program]
mod quanductor {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {
        ctx.accounts.handler(&ctx.bumps)
    }
}

#[cfg(test)]
mod tests;
