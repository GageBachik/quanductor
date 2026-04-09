extern crate std;

use quasar_svm::{Account, Instruction, Pubkey, QuasarSvm};
use solana_address::Address;
use solana_instruction::AccountMeta;
use std::vec;
use std::vec::Vec;

use quanductor_client::{
    ComputeThresholdInstruction, CrankScoresInstruction, DelegateStakeInstruction,
    InitializeInstruction, UndelegateStakeInstruction,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const STAKE_PROGRAM_ID: [u8; 32] = [
    6u8, 161, 216, 23, 145, 55, 84, 42, 152, 52, 55, 189, 254, 42, 122, 178, 85, 127, 83, 92,
    138, 120, 114, 43, 104, 164, 157, 192, 0, 0, 0, 0,
];

const VH_DISCRIMINATOR: [u8; 8] = [205, 25, 8, 221, 253, 131, 2, 146];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn vh_program_id() -> Pubkey {
    // HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa
    Pubkey::from([
        248u8, 117, 89, 98, 214, 59, 7, 171, 71, 19, 73, 145, 33, 110, 50, 221,
        143, 183, 121, 232, 10, 6, 93, 249, 125, 111, 126, 136, 142, 31, 12, 245,
    ])
}

fn setup() -> QuasarSvm {
    let elf = include_bytes!("../target/deploy/quanductor.so");
    QuasarSvm::new().with_program(&Pubkey::from(crate::ID), elf)
}

fn scoring_state_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"scoring_state"], &Pubkey::from(crate::ID))
}

fn stake_authority_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"stake_authority"], &Pubkey::from(crate::ID))
}

fn signer(address: Pubkey) -> Account {
    Account {
        address,
        lamports: 10_000_000_000,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

fn empty(address: Pubkey) -> Account {
    Account {
        address,
        lamports: 0,
        data: vec![],
        owner: quasar_svm::system_program::ID,
        executable: false,
    }
}

fn mock_validator_history(
    address: Pubkey,
    index: u32,
    vote_account: &Pubkey,
    entries: &[(u16, u32, u8)], // (epoch, epoch_credits, commission)
) -> Account {
    let total_size: usize = 65_864;
    let mut data = vec![0u8; total_size];

    // Discriminator
    data[0..8].copy_from_slice(&VH_DISCRIMINATOR);
    // struct_version = 1
    data[8..12].copy_from_slice(&1u32.to_le_bytes());
    // vote_account (32 bytes at offset 12)
    data[12..44].copy_from_slice(vote_account.as_ref());
    // index (u32 at offset 44)
    data[44..48].copy_from_slice(&index.to_le_bytes());

    // CircBuf header
    if entries.is_empty() {
        // history.idx = 0, history.is_empty = 1
        data[304..312].copy_from_slice(&0u64.to_le_bytes());
        data[312] = 1; // is_empty
    } else {
        let last_idx = (entries.len() - 1) as u64;
        data[304..312].copy_from_slice(&last_idx.to_le_bytes());
        data[312] = 0; // not empty
    }

    // Write entries starting at byte 320, each 128 bytes
    for (i, &(epoch, epoch_credits, commission)) in entries.iter().enumerate() {
        let base = 320 + i * 128;
        // epoch at entry_offset + 8
        data[base + 8..base + 10].copy_from_slice(&epoch.to_le_bytes());
        // epoch_credits at entry_offset + 12
        data[base + 12..base + 16].copy_from_slice(&epoch_credits.to_le_bytes());
        // commission at entry_offset + 16
        data[base + 16] = commission;
    }

    // Fill unused entries with sentinel values
    for i in entries.len()..512 {
        let base = 320 + i * 128;
        // epoch = u16::MAX
        data[base + 8..base + 10].copy_from_slice(&u16::MAX.to_le_bytes());
        // commission = u8::MAX
        data[base + 16] = u8::MAX;
    }

    Account {
        address,
        lamports: 10_000_000,
        data,
        owner: vh_program_id(),
        executable: false,
    }
}

fn mock_scoring_state_computed(
    address: Pubkey,
    epoch: u64,
    threshold: u64,
    stake_authority_bump: u8,
    bump: u8,
) -> Account {
    let total_size = 1 + 1 + 8 + 8 + 2 + 1024 + 768 + 1 + 1; // 1814
    let mut data = vec![0u8; total_size];

    data[0] = 1; // discriminator
    data[1] = 2; // phase = THRESHOLD_COMPUTED
    data[2..10].copy_from_slice(&epoch.to_le_bytes());
    data[10..18].copy_from_slice(&threshold.to_le_bytes());
    data[18..20].copy_from_slice(&1500u16.to_le_bytes()); // total_scored
    // histogram and bitmap stay zeroed
    data[1812] = stake_authority_bump;
    data[1813] = bump;

    Account {
        address,
        lamports: 10_000_000,
        data,
        owner: Pubkey::from(crate::ID),
        executable: false,
    }
}

fn mock_scoring_state_idle(address: Pubkey, bump: u8) -> Account {
    let total_size = 1 + 1 + 8 + 8 + 2 + 1024 + 768 + 1 + 1;
    let mut data = vec![0u8; total_size];

    data[0] = 1; // discriminator
    data[1] = 0; // phase = IDLE
    data[1813] = bump;

    Account {
        address,
        lamports: 10_000_000,
        data,
        owner: Pubkey::from(crate::ID),
        executable: false,
    }
}

fn mock_scoring_state_cranking(address: Pubkey, epoch: u64, bump: u8) -> Account {
    let total_size = 1 + 1 + 8 + 8 + 2 + 1024 + 768 + 1 + 1;
    let mut data = vec![0u8; total_size];

    data[0] = 1; // discriminator
    data[1] = 1; // phase = CRANKING
    data[2..10].copy_from_slice(&epoch.to_le_bytes());
    data[1813] = bump;

    Account {
        address,
        lamports: 10_000_000,
        data,
        owner: Pubkey::from(crate::ID),
        executable: false,
    }
}

fn mock_stake_initialized(address: Pubkey, staker: &Pubkey) -> Account {
    let mut data = vec![0u8; 200];
    data[0..4].copy_from_slice(&1u32.to_le_bytes()); // Initialized
    data[12..44].copy_from_slice(staker.as_ref()); // staker

    Account {
        address,
        lamports: 5_000_000_000,
        data,
        owner: Pubkey::from(STAKE_PROGRAM_ID),
        executable: false,
    }
}

fn mock_stake_active(address: Pubkey, staker: &Pubkey, voter: &Pubkey) -> Account {
    let mut data = vec![0u8; 200];
    data[0..4].copy_from_slice(&2u32.to_le_bytes()); // Stake
    data[12..44].copy_from_slice(staker.as_ref()); // staker
    data[124..156].copy_from_slice(voter.as_ref()); // voter
    // deactivation_epoch = u64::MAX (active)
    data[172..180].copy_from_slice(&u64::MAX.to_le_bytes());

    Account {
        address,
        lamports: 5_000_000_000,
        data,
        owner: Pubkey::from(STAKE_PROGRAM_ID),
        executable: false,
    }
}

/// Run initialize and return the SVM with scoring_state created
fn initialize_scoring_state(svm: &mut QuasarSvm) -> Pubkey {
    let payer = Pubkey::new_unique();
    let (scoring_state, _) = scoring_state_pda();

    let ix: Instruction = InitializeInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        system_program: Address::from(quasar_svm::system_program::ID.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(&ix, &[signer(payer), empty(scoring_state)]);
    result.assert_success();
    scoring_state
}

// ---------------------------------------------------------------------------
// Initialize Tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let mut svm = setup();

    let payer = Pubkey::new_unique();
    let (scoring_state, _) = scoring_state_pda();

    let ix: Instruction = InitializeInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        system_program: Address::from(quasar_svm::system_program::ID.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(&ix, &[signer(payer), empty(scoring_state)]);
    result.assert_success();

    let scoring_account = result.account(&scoring_state).unwrap();
    assert_eq!(scoring_account.data[0], 1, "discriminator should be 1");
    assert_eq!(scoring_account.data[1], 0, "phase should be PHASE_IDLE (0)");
}

#[test]
fn test_initialize_double_init_fails() {
    let mut svm = setup();

    let payer = Pubkey::new_unique();
    let (scoring_state, _) = scoring_state_pda();

    let ix: Instruction = InitializeInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        system_program: Address::from(quasar_svm::system_program::ID.to_bytes()),
    }
    .into();

    // First init succeeds
    let result = svm.process_instruction(&ix, &[signer(payer), empty(scoring_state)]);
    result.assert_success();

    // Second init should fail — account already exists (pass the existing account from SVM)
    let result = svm.process_instruction(
        &ix,
        &[signer(payer), svm.get_account(&scoring_state).unwrap()],
    );
    assert!(result.is_err(), "double init should fail");
}

// ---------------------------------------------------------------------------
// Crank Scores Tests
// ---------------------------------------------------------------------------

#[test]
fn test_crank_single_validator() {
    let mut svm = setup();
    let scoring_state = initialize_scoring_state(&mut svm);
    let payer = Pubkey::new_unique();

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();

    // Current epoch in SVM defaults to 0; set it to something meaningful
    svm.sysvars.clock.epoch = 100;

    // Create a VH with 5 epochs of data
    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 400_000u32, 5u8)).collect();
    let vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);

    let ix: Instruction = CrankScoresInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        remaining_accounts: vec![AccountMeta::new_readonly(
            solana_pubkey::Pubkey::from(vh_addr.to_bytes()),
            false,
        )],
    }
    .into();

    let result = svm.process_instruction(&ix, &[signer(payer), svm.get_account(&scoring_state).unwrap(), vh_account]);
    result.assert_success();

    let state = result.account(&scoring_state).unwrap();
    // Phase should be CRANKING (1) — epoch changed from 0 to 100, triggers reset
    assert_eq!(state.data[1], 1, "phase should be PHASE_CRANKING");
    // total_scored should be 1
    let total_scored = u16::from_le_bytes([state.data[18], state.data[19]]);
    assert_eq!(total_scored, 1, "total_scored should be 1");
}

#[test]
fn test_crank_duplicate_skipped() {
    let mut svm = setup();
    let scoring_state = initialize_scoring_state(&mut svm);
    let payer = Pubkey::new_unique();

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();

    svm.sysvars.clock.epoch = 100;

    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 400_000u32, 5u8)).collect();
    let vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);

    let ix: Instruction = CrankScoresInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        remaining_accounts: vec![AccountMeta::new_readonly(
            solana_pubkey::Pubkey::from(vh_addr.to_bytes()),
            false,
        )],
    }
    .into();

    // First crank
    let result = svm.process_instruction(&ix, &[signer(payer), svm.get_account(&scoring_state).unwrap(), vh_account.clone()]);
    result.assert_success();

    // Second crank with same validator — should succeed but not increment
    let result2 = svm.process_instruction(&ix, &[signer(payer), svm.get_account(&scoring_state).unwrap(), vh_account]);
    result2.assert_success();

    let state = result2.account(&scoring_state).unwrap();
    let total_scored = u16::from_le_bytes([state.data[18], state.data[19]]);
    assert_eq!(total_scored, 1, "total_scored should still be 1 after duplicate");
}

#[test]
fn test_crank_invalid_owner_rejected() {
    let mut svm = setup();
    let scoring_state = initialize_scoring_state(&mut svm);
    let payer = Pubkey::new_unique();

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();

    svm.sysvars.clock.epoch = 100;

    // Build a VH account but with wrong owner
    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 400_000u32, 5u8)).collect();
    let mut vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);
    vh_account.owner = Pubkey::new_unique(); // wrong owner

    let ix: Instruction = CrankScoresInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        remaining_accounts: vec![AccountMeta::new_readonly(
            solana_pubkey::Pubkey::from(vh_addr.to_bytes()),
            false,
        )],
    }
    .into();

    let result = svm.process_instruction(&ix, &[signer(payer), svm.get_account(&scoring_state).unwrap(), vh_account]);
    assert!(result.is_err(), "should reject VH with wrong owner");
}

#[test]
fn test_epoch_transition_resets() {
    let mut svm = setup();
    let scoring_state = initialize_scoring_state(&mut svm);
    let payer = Pubkey::new_unique();

    // First epoch: crank one validator
    svm.sysvars.clock.epoch = 100;

    let vote_account1 = Pubkey::new_unique();
    let vh_addr1 = Pubkey::new_unique();
    let entries1: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 400_000u32, 5u8)).collect();
    let vh1 = mock_validator_history(vh_addr1, 0, &vote_account1, &entries1);

    let ix1: Instruction = CrankScoresInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        remaining_accounts: vec![AccountMeta::new_readonly(
            solana_pubkey::Pubkey::from(vh_addr1.to_bytes()),
            false,
        )],
    }
    .into();

    let result = svm.process_instruction(&ix1, &[signer(payer), svm.get_account(&scoring_state).unwrap(), vh1]);
    result.assert_success();

    // Verify total_scored = 1 in epoch 100
    let state = result.account(&scoring_state).unwrap();
    let total_scored = u16::from_le_bytes([state.data[18], state.data[19]]);
    assert_eq!(total_scored, 1, "total_scored should be 1 in first epoch");

    // Change epoch — crank a different validator
    svm.sysvars.clock.epoch = 101;

    let vote_account2 = Pubkey::new_unique();
    let vh_addr2 = Pubkey::new_unique();
    let entries2: Vec<(u16, u32, u8)> = (97..=101).map(|e| (e as u16, 300_000u32, 10u8)).collect();
    let vh2 = mock_validator_history(vh_addr2, 1, &vote_account2, &entries2);

    let ix2: Instruction = CrankScoresInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        remaining_accounts: vec![AccountMeta::new_readonly(
            solana_pubkey::Pubkey::from(vh_addr2.to_bytes()),
            false,
        )],
    }
    .into();

    let result2 = svm.process_instruction(&ix2, &[signer(payer), svm.get_account(&scoring_state).unwrap(), vh2]);
    result2.assert_success();

    let state2 = result2.account(&scoring_state).unwrap();
    // Epoch should be updated to 101
    let epoch = u64::from_le_bytes(state2.data[2..10].try_into().unwrap());
    assert_eq!(epoch, 101, "epoch should be updated to 101");
    // total_scored should reset to 1 (only the new validator)
    let total_scored2 = u16::from_le_bytes([state2.data[18], state2.data[19]]);
    assert_eq!(total_scored2, 1, "total_scored should reset to 1 after epoch change");
}

// ---------------------------------------------------------------------------
// Compute Threshold Tests
// ---------------------------------------------------------------------------

#[test]
fn test_compute_threshold_insufficient_validators() {
    let mut svm = setup();
    let (scoring_state, bump) = scoring_state_pda();

    // Create a CRANKING state with only 50 validators scored (need 100)
    let total_size = 1 + 1 + 8 + 8 + 2 + 1024 + 768 + 1 + 1;
    let mut data = vec![0u8; total_size];
    data[0] = 1; // discriminator
    data[1] = 1; // CRANKING
    data[2..10].copy_from_slice(&100u64.to_le_bytes()); // epoch
    data[18..20].copy_from_slice(&50u16.to_le_bytes()); // total_scored = 50
    data[1813] = bump;

    let state_account = Account {
        address: scoring_state,
        lamports: 10_000_000,
        data,
        owner: Pubkey::from(crate::ID),
        executable: false,
    };

    svm.sysvars.clock.epoch = 100;

    let ix: Instruction = ComputeThresholdInstruction {
        scoring_state: Address::from(scoring_state.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(&ix, &[state_account]);
    assert!(result.is_err(), "should fail with insufficient validators");
}

#[test]
fn test_compute_threshold_wrong_phase() {
    let mut svm = setup();
    let (scoring_state, bump) = scoring_state_pda();

    // Create an IDLE state account
    let state_account = mock_scoring_state_idle(scoring_state, bump);

    let ix: Instruction = ComputeThresholdInstruction {
        scoring_state: Address::from(scoring_state.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(&ix, &[state_account]);
    assert!(result.is_err(), "should fail when phase is IDLE");
}

// ---------------------------------------------------------------------------
// Delegate Stake Tests
// ---------------------------------------------------------------------------

#[test]
fn test_delegate_wrong_phase() {
    let mut svm = setup();
    let (scoring_state, bump) = scoring_state_pda();
    let (stake_authority, _sa_bump) = stake_authority_pda();

    svm.sysvars.clock.epoch = 100;

    // Use CRANKING phase instead of THRESHOLD_COMPUTED
    let state_account = mock_scoring_state_cranking(scoring_state, 100, bump);

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();
    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 400_000u32, 5u8)).collect();
    let vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);

    let stake_addr = Pubkey::new_unique();
    let stake_account = mock_stake_initialized(stake_addr, &stake_authority);

    let clock_sysvar = Pubkey::from([
        6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184,
        163, 155, 75, 109, 92, 115, 85, 91, 42, 0, 0, 0, 0,
    ]);
    let stake_history_sysvar = Pubkey::from([
        6, 167, 213, 23, 25, 53, 132, 208, 254, 237, 155, 179, 67, 29, 19, 32, 107, 229, 68,
        40, 27, 87, 184, 86, 108, 197, 55, 95, 0, 0, 0, 0,
    ]);
    let stake_config = Pubkey::from([
        6, 161, 216, 23, 165, 2, 5, 11, 104, 7, 145, 230, 206, 109, 184, 142, 30, 91, 137,
        68, 246, 131, 148, 21, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    let stake_program = Pubkey::from(STAKE_PROGRAM_ID);

    let ix: Instruction = DelegateStakeInstruction {
        scoring_state: Address::from(scoring_state.to_bytes()),
        stake_account: Address::from(stake_addr.to_bytes()),
        validator_history: Address::from(vh_addr.to_bytes()),
        validator_vote_account: Address::from(vote_account.to_bytes()),
        clock_sysvar: Address::from(clock_sysvar.to_bytes()),
        stake_history_sysvar: Address::from(stake_history_sysvar.to_bytes()),
        stake_config: Address::from(stake_config.to_bytes()),
        stake_authority: Address::from(stake_authority.to_bytes()),
        stake_program: Address::from(stake_program.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(
        &ix,
        &[
            state_account,
            stake_account,
            vh_account,
            empty(vote_account),
            empty(clock_sysvar),
            empty(stake_history_sysvar),
            empty(stake_config),
            empty(stake_authority),
            empty(stake_program),
        ],
    );

    assert!(result.is_err(), "delegate should fail when phase is CRANKING");
}

#[test]
fn test_delegate_below_threshold() {
    let mut svm = setup();
    let (scoring_state, bump) = scoring_state_pda();
    let (stake_authority, sa_bump) = stake_authority_pda();

    svm.sysvars.clock.epoch = 100;

    // Set a high threshold that the validator won't meet
    let state_account = mock_scoring_state_computed(scoring_state, 100, 500_000, sa_bump, bump);

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();
    // Low credits = low score, well below threshold of 500_000
    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 100u32, 5u8)).collect();
    let vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);

    let stake_addr = Pubkey::new_unique();
    let stake_account = mock_stake_initialized(stake_addr, &stake_authority);

    let clock_sysvar = Pubkey::from([
        6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184,
        163, 155, 75, 109, 92, 115, 85, 91, 42, 0, 0, 0, 0,
    ]);
    let stake_history_sysvar = Pubkey::from([
        6, 167, 213, 23, 25, 53, 132, 208, 254, 237, 155, 179, 67, 29, 19, 32, 107, 229, 68,
        40, 27, 87, 184, 86, 108, 197, 55, 95, 0, 0, 0, 0,
    ]);
    let stake_config = Pubkey::from([
        6, 161, 216, 23, 165, 2, 5, 11, 104, 7, 145, 230, 206, 109, 184, 142, 30, 91, 137,
        68, 246, 131, 148, 21, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    let stake_program = Pubkey::from(STAKE_PROGRAM_ID);

    let ix: Instruction = DelegateStakeInstruction {
        scoring_state: Address::from(scoring_state.to_bytes()),
        stake_account: Address::from(stake_addr.to_bytes()),
        validator_history: Address::from(vh_addr.to_bytes()),
        validator_vote_account: Address::from(vote_account.to_bytes()),
        clock_sysvar: Address::from(clock_sysvar.to_bytes()),
        stake_history_sysvar: Address::from(stake_history_sysvar.to_bytes()),
        stake_config: Address::from(stake_config.to_bytes()),
        stake_authority: Address::from(stake_authority.to_bytes()),
        stake_program: Address::from(stake_program.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(
        &ix,
        &[
            state_account,
            stake_account,
            vh_account,
            empty(vote_account),
            empty(clock_sysvar),
            empty(stake_history_sysvar),
            empty(stake_config),
            empty(stake_authority),
            empty(stake_program),
        ],
    );

    assert!(result.is_err(), "delegate should fail when score is below threshold");
}

// ---------------------------------------------------------------------------
// Undelegate Stake Tests
// ---------------------------------------------------------------------------

#[test]
fn test_undelegate_wrong_phase() {
    let mut svm = setup();
    let (scoring_state, bump) = scoring_state_pda();
    let (stake_authority, _sa_bump) = stake_authority_pda();

    svm.sysvars.clock.epoch = 100;

    // Use CRANKING phase instead of THRESHOLD_COMPUTED
    let state_account = mock_scoring_state_cranking(scoring_state, 100, bump);

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();
    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 100u32, 5u8)).collect();
    let vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);

    let stake_addr = Pubkey::new_unique();
    let stake_account = mock_stake_active(stake_addr, &stake_authority, &vote_account);

    let clock_sysvar = Pubkey::from([
        6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184,
        163, 155, 75, 109, 92, 115, 85, 91, 42, 0, 0, 0, 0,
    ]);
    let stake_program = Pubkey::from(STAKE_PROGRAM_ID);

    let ix: Instruction = UndelegateStakeInstruction {
        scoring_state: Address::from(scoring_state.to_bytes()),
        stake_account: Address::from(stake_addr.to_bytes()),
        validator_history: Address::from(vh_addr.to_bytes()),
        clock_sysvar: Address::from(clock_sysvar.to_bytes()),
        stake_authority: Address::from(stake_authority.to_bytes()),
        stake_program: Address::from(stake_program.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(
        &ix,
        &[
            state_account,
            stake_account,
            vh_account,
            empty(clock_sysvar),
            empty(stake_authority),
            empty(stake_program),
        ],
    );

    assert!(result.is_err(), "undelegate should fail when phase is CRANKING");
}

#[test]
fn test_undelegate_above_threshold() {
    let mut svm = setup();
    let (scoring_state, bump) = scoring_state_pda();
    let (stake_authority, sa_bump) = stake_authority_pda();

    svm.sysvars.clock.epoch = 100;

    // Set a low threshold — validator with high score should NOT be undelegated
    let state_account = mock_scoring_state_computed(scoring_state, 100, 10, sa_bump, bump);

    let vote_account = Pubkey::new_unique();
    let vh_addr = Pubkey::new_unique();
    // High credits = high score, well above threshold of 10
    let entries: Vec<(u16, u32, u8)> = (96..=100).map(|e| (e as u16, 400_000u32, 5u8)).collect();
    let vh_account = mock_validator_history(vh_addr, 0, &vote_account, &entries);

    let stake_addr = Pubkey::new_unique();
    let stake_account = mock_stake_active(stake_addr, &stake_authority, &vote_account);

    let clock_sysvar = Pubkey::from([
        6, 167, 213, 23, 24, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184,
        163, 155, 75, 109, 92, 115, 85, 91, 42, 0, 0, 0, 0,
    ]);
    let stake_program = Pubkey::from(STAKE_PROGRAM_ID);

    let ix: Instruction = UndelegateStakeInstruction {
        scoring_state: Address::from(scoring_state.to_bytes()),
        stake_account: Address::from(stake_addr.to_bytes()),
        validator_history: Address::from(vh_addr.to_bytes()),
        clock_sysvar: Address::from(clock_sysvar.to_bytes()),
        stake_authority: Address::from(stake_authority.to_bytes()),
        stake_program: Address::from(stake_program.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(
        &ix,
        &[
            state_account,
            stake_account,
            vh_account,
            empty(clock_sysvar),
            empty(stake_authority),
            empty(stake_program),
        ],
    );

    assert!(result.is_err(), "undelegate should fail when score is above threshold");
}
