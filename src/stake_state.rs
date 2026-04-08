/// Helpers for parsing native Solana stake account data.
///
/// Stake account layout (relevant offsets):
/// - 0..4:    state enum (u32 LE)
/// - 4..12:   rent_exempt_reserve (u64 LE)
/// - 12..44:  authorized.staker (Pubkey)
/// - 44..76:  authorized.withdrawer (Pubkey)
/// - 76..124: lockup fields
/// - 124..156: delegation.voter_pubkey (Pubkey) — only when state == 2
/// - 156..164: delegation.stake (u64 LE)
/// - 164..172: delegation.activation_epoch (u64 LE)
/// - 172..180: delegation.deactivation_epoch (u64 LE)

pub const STAKE_STATE_UNINITIALIZED: u32 = 0;
pub const STAKE_STATE_INITIALIZED: u32 = 1;
pub const STAKE_STATE_STAKE: u32 = 2;

/// Read the stake state enum (u32 LE) at offset 0.
#[inline(always)]
pub fn read_stake_state(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Return a reference to the 32-byte staker pubkey at offset 12.
#[inline(always)]
pub fn read_staker(data: &[u8]) -> &[u8; 32] {
    // SAFETY: We need at least 44 bytes (12 + 32). The caller must ensure
    // the slice is large enough (any valid stake account is well over 44 bytes).
    unsafe { &*(data.as_ptr().add(12) as *const [u8; 32]) }
}

/// Return a reference to the 32-byte voter pubkey at offset 124.
/// Only valid when state == STAKE_STATE_STAKE.
#[inline(always)]
pub fn read_voter(data: &[u8]) -> &[u8; 32] {
    unsafe { &*(data.as_ptr().add(124) as *const [u8; 32]) }
}

/// Read the deactivation epoch (u64 LE) at offset 172.
/// Only valid when state == STAKE_STATE_STAKE.
#[inline(always)]
pub fn read_deactivation_epoch(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[172], data[173], data[174], data[175],
        data[176], data[177], data[178], data[179],
    ])
}

/// Returns true if the stake account can be delegated:
/// - State is Initialized, OR
/// - State is Stake AND deactivation_epoch < current_epoch (fully deactivated).
#[inline(always)]
pub fn is_delegatable(data: &[u8], current_epoch: u64) -> bool {
    let state = read_stake_state(data);
    if state == STAKE_STATE_INITIALIZED {
        return true;
    }
    if state == STAKE_STATE_STAKE {
        return read_deactivation_epoch(data) < current_epoch;
    }
    false
}

/// Returns true if the stake account is actively staked:
/// State is Stake AND deactivation_epoch == u64::MAX.
#[inline(always)]
pub fn is_active(data: &[u8]) -> bool {
    read_stake_state(data) == STAKE_STATE_STAKE
        && read_deactivation_epoch(data) == u64::MAX
}
