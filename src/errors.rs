use quasar_lang::prelude::*;

#[error_code]
pub enum QuanductorError {
    InvalidPhase,
    EpochMismatch,
    InsufficientValidators,
    ScoreBelowThreshold,
    ScoreAboveThreshold,
    InvalidStakeState,
    InvalidStakeAuthority,
    InvalidValidatorHistory,
    InsufficientEpochData,
    InvalidVoteAccount,
}
