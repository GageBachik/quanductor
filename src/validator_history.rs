use quasar_lang::prelude::*;

use crate::state::SCORE_RANGE;

// --- Program ID ---
// HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa
pub const VH_PROGRAM_ID: Address = Address::new_from_array([
    248u8, 117, 89, 98, 214, 59, 7, 171, 71, 19, 73, 145, 33, 110, 50, 221,
    143, 183, 121, 232, 10, 6, 93, 249, 125, 111, 126, 136, 142, 31, 12, 245,
]);

// --- Discriminator ---
pub const VH_DISCRIMINATOR: [u8; 8] = [205, 25, 8, 221, 253, 131, 2, 146];

// --- Offsets ---
pub const VH_OFFSET_STRUCT_VERSION: usize = 8;
pub const VH_OFFSET_VOTE_ACCOUNT: usize = 12;
pub const VH_OFFSET_INDEX: usize = 44;
pub const VH_OFFSET_BUMP: usize = 48;
pub const VH_OFFSET_LAST_IP_TIMESTAMP: usize = 56;
pub const VH_OFFSET_LAST_VERSION_TIMESTAMP: usize = 64;
pub const VH_OFFSET_VALIDATOR_AGE: usize = 72;
pub const VH_OFFSET_VALIDATOR_AGE_LAST_UPDATED_EPOCH: usize = 76;
pub const VH_OFFSET_HISTORY: usize = 304;

// --- CircBuf offsets (relative to account start) ---
pub const VH_OFFSET_CIRC_BUF_IDX: usize = 304;
pub const VH_OFFSET_CIRC_BUF_IS_EMPTY: usize = 312;
pub const VH_OFFSET_CIRC_BUF_ARR: usize = 320;

// --- Entry layout ---
pub const ENTRY_SIZE: usize = 128;
pub const MAX_ENTRIES: usize = 512;

// Entry field offsets (relative to entry start)
pub const ENTRY_OFFSET_ACTIVATED_STAKE: usize = 0;
pub const ENTRY_OFFSET_EPOCH: usize = 8;
pub const ENTRY_OFFSET_MEV_COMMISSION: usize = 10;
pub const ENTRY_OFFSET_EPOCH_CREDITS: usize = 12;
pub const ENTRY_OFFSET_COMMISSION: usize = 16;

// --- Sentinel values ---
pub const EPOCH_UNSET: u16 = u16::MAX;
pub const COMMISSION_UNSET: u8 = u8::MAX;

// --- Minimum data length ---
pub const VH_MIN_DATA_LEN: usize = 65_864;

/// Validate that the account data belongs to ValidatorHistory.
/// Checks: owner matches expected program, discriminator matches, data length sufficient.
#[inline(always)]
pub fn validate_validator_history(
    data: &[u8],
    owner: &Address,
    expected_owner: &Address,
) -> Result<(), ProgramError> {
    // Check owner
    if owner != expected_owner {
        return Err(ProgramError::IllegalOwner);
    }
    // Check minimum data length
    if data.len() < VH_MIN_DATA_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    // Check discriminator
    if data[0..8] != VH_DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

/// Read the validator index (u32) at offset 44.
#[inline(always)]
pub fn read_vh_index(data: &[u8]) -> u32 {
    let bytes: [u8; 4] = [
        data[VH_OFFSET_INDEX],
        data[VH_OFFSET_INDEX + 1],
        data[VH_OFFSET_INDEX + 2],
        data[VH_OFFSET_INDEX + 3],
    ];
    u32::from_le_bytes(bytes)
}

/// Read the vote account (32 bytes) at offset 12.
#[inline(always)]
pub fn read_vh_vote_account(data: &[u8]) -> &[u8; 32] {
    // SAFETY: we trust data.len() >= VH_MIN_DATA_LEN after validation
    let ptr = &data[VH_OFFSET_VOTE_ACCOUNT..VH_OFFSET_VOTE_ACCOUNT + 32];
    unsafe { &*(ptr.as_ptr() as *const [u8; 32]) }
}

/// Read the circular buffer index (u64) at offset 304.
#[inline(always)]
pub fn read_circ_buf_idx(data: &[u8]) -> u64 {
    let off = VH_OFFSET_CIRC_BUF_IDX;
    let bytes: [u8; 8] = [
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ];
    u64::from_le_bytes(bytes)
}

/// Check if the circular buffer is empty (byte at offset 312).
#[inline(always)]
pub fn is_circ_buf_empty(data: &[u8]) -> bool {
    data[VH_OFFSET_CIRC_BUF_IS_EMPTY] != 0
}

/// Compute the byte offset of a given entry in the circular buffer.
#[inline(always)]
fn entry_offset(entry_idx: usize) -> usize {
    VH_OFFSET_CIRC_BUF_ARR + entry_idx * ENTRY_SIZE
}

/// Read the epoch (u16) from a specific entry.
#[inline(always)]
pub fn read_entry_epoch(data: &[u8], entry_idx: usize) -> u16 {
    let off = entry_offset(entry_idx) + ENTRY_OFFSET_EPOCH;
    let bytes: [u8; 2] = [data[off], data[off + 1]];
    u16::from_le_bytes(bytes)
}

/// Read the epoch_credits (u32) from a specific entry.
#[inline(always)]
pub fn read_entry_epoch_credits(data: &[u8], entry_idx: usize) -> u32 {
    let off = entry_offset(entry_idx) + ENTRY_OFFSET_EPOCH_CREDITS;
    let bytes: [u8; 4] = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    u32::from_le_bytes(bytes)
}

/// Read the commission (u8) from a specific entry.
#[inline(always)]
pub fn read_entry_commission(data: &[u8], entry_idx: usize) -> u8 {
    let off = entry_offset(entry_idx) + ENTRY_OFFSET_COMMISSION;
    data[off]
}

/// Compute an average reward score over the last `lookback` epochs.
///
/// Walks backwards through the circular buffer from the current position.
/// For each valid entry in the epoch range, computes:
///   reward = epoch_credits * (100 - commission) / 100
/// Returns total_reward / valid_epochs, or 0 if no valid data.
pub fn compute_score(data: &[u8], current_epoch: u64, lookback: usize) -> u64 {
    if is_circ_buf_empty(data) {
        return 0;
    }

    let circ_idx = read_circ_buf_idx(data) as usize;
    let min_epoch = current_epoch.saturating_sub(lookback as u64 - 1);

    let mut total_reward: u64 = 0;
    let mut valid_epochs: u64 = 0;

    let mut i: usize = 0;
    while i < MAX_ENTRIES {
        // Walk backwards from circ_idx, wrapping around
        let idx = if circ_idx >= i {
            circ_idx - i
        } else {
            MAX_ENTRIES + circ_idx - i
        };

        let epoch = read_entry_epoch(data, idx) as u64;

        // Skip unset entries
        if epoch == EPOCH_UNSET as u64 {
            i += 1;
            continue;
        }

        // Stop if we've gone past the lookback window
        if epoch < min_epoch {
            break;
        }

        // Only consider entries in the valid range
        if epoch <= current_epoch {
            let commission = read_entry_commission(data, idx);

            // Skip unset commission
            if commission != COMMISSION_UNSET {
                let credits = read_entry_epoch_credits(data, idx) as u64;
                let reward = credits * (100 - commission as u64) / 100;
                total_reward += reward;
                valid_epochs += 1;
            }

            // Stop early if we've found enough valid epochs
            if valid_epochs as usize >= lookback {
                break;
            }
        }

        i += 1;
    }

    if valid_epochs == 0 {
        0
    } else {
        total_reward / valid_epochs
    }
}

/// Map a score to a histogram bucket index (0..511).
/// Formula: min(score * 512 / SCORE_RANGE, 511)
#[inline(always)]
pub fn score_to_bucket(score: u64) -> usize {
    let bucket = score * 512 / SCORE_RANGE;
    if bucket > 511 {
        511
    } else {
        bucket as usize
    }
}

/// Check if a bit is set in a bitmap byte array.
/// The bit at `index` is stored at byte `index / 8`, bit `index % 8`.
#[inline(always)]
pub fn bitmap_is_set(bitmap: &[u8], index: u32) -> bool {
    let byte_idx = (index / 8) as usize;
    let bit_idx = index % 8;
    if byte_idx >= bitmap.len() {
        return false;
    }
    (bitmap[byte_idx] >> bit_idx) & 1 == 1
}

/// Set a bit in a bitmap byte array.
#[inline(always)]
pub fn bitmap_set(bitmap: &mut [u8], index: u32) {
    let byte_idx = (index / 8) as usize;
    let bit_idx = index % 8;
    if byte_idx < bitmap.len() {
        bitmap[byte_idx] |= 1 << bit_idx;
    }
}
