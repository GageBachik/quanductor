/// CPI builders for native Solana Stake program instructions.

use quasar_lang::{
    cpi::{CpiCall, InstructionAccount, Seed},
    prelude::{AccountView, Address, ProgramError},
};

/// Native Stake program ID: `Stake11111111111111111111111111111111111111`
pub const STAKE_PROGRAM_ID: Address = Address::new_from_array([
    6, 161, 216, 23, 145, 55, 84, 42, 152, 52, 55, 189, 254, 42, 122, 178,
    85, 127, 83, 92, 138, 120, 114, 43, 104, 164, 157, 192, 0, 0, 0, 0,
]);

/// Issue a `DelegateStake` CPI to the native Stake program.
///
/// StakeInstruction::DelegateStake = index 2 (4-byte LE u32).
///
/// Accounts:
///   0. `[WRITE]`  stake_account
///   1. `[]`       vote_account
///   2. `[]`       clock sysvar
///   3. `[]`       stake_history sysvar
///   4. `[]`       stake_config (deprecated but required)
///   5. `[SIGNER]` stake_authority
pub fn delegate_stake_cpi<'a>(
    stake_program: &'a AccountView,
    stake_account: &'a AccountView,
    vote_account: &'a AccountView,
    clock: &'a AccountView,
    stake_history: &'a AccountView,
    stake_config: &'a AccountView,
    stake_authority: &'a AccountView,
    seeds: &[Seed],
) -> Result<(), ProgramError> {
    CpiCall::<6, 4>::new(
        stake_program.address(),
        [
            InstructionAccount::writable(stake_account.address()),
            InstructionAccount::readonly(vote_account.address()),
            InstructionAccount::readonly(clock.address()),
            InstructionAccount::readonly(stake_history.address()),
            InstructionAccount::readonly(stake_config.address()),
            InstructionAccount::readonly_signer(stake_authority.address()),
        ],
        [stake_account, vote_account, clock, stake_history, stake_config, stake_authority],
        [2u8, 0, 0, 0],
    )
    .invoke_signed(seeds)
}

/// Issue a `Deactivate` CPI to the native Stake program.
///
/// StakeInstruction::Deactivate = index 5 (4-byte LE u32).
///
/// Accounts:
///   0. `[WRITE]`  stake_account
///   1. `[]`       clock sysvar
///   2. `[SIGNER]` stake_authority
pub fn deactivate_stake_cpi<'a>(
    stake_program: &'a AccountView,
    stake_account: &'a AccountView,
    clock: &'a AccountView,
    stake_authority: &'a AccountView,
    seeds: &[Seed],
) -> Result<(), ProgramError> {
    CpiCall::<3, 4>::new(
        stake_program.address(),
        [
            InstructionAccount::writable(stake_account.address()),
            InstructionAccount::readonly(clock.address()),
            InstructionAccount::readonly_signer(stake_authority.address()),
        ],
        [stake_account, clock, stake_authority],
        [5u8, 0, 0, 0],
    )
    .invoke_signed(seeds)
}
