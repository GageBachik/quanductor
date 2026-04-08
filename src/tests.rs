extern crate std;

use quasar_svm::{Account, Instruction, Pubkey, QuasarSvm};
use solana_address::Address;
use std::vec;

use quanductor_client::InitializeInstruction;

fn setup() -> QuasarSvm {
    let elf = include_bytes!("../target/deploy/quanductor.so");
    QuasarSvm::new().with_program(&Pubkey::from(crate::ID), elf)
}

#[test]
fn test_initialize() {
    let mut svm = setup();

    let payer = Pubkey::new_unique();
    let (scoring_state, _) =
        Pubkey::find_program_address(&[b"scoring_state"], &Pubkey::from(crate::ID));

    let instruction: Instruction = InitializeInstruction {
        payer: Address::from(payer.to_bytes()),
        scoring_state: Address::from(scoring_state.to_bytes()),
        system_program: Address::from(quasar_svm::system_program::ID.to_bytes()),
    }
    .into();

    let result = svm.process_instruction(
        &instruction,
        &[
            Account {
                address: payer,
                lamports: 10_000_000_000,
                data: vec![],
                owner: quasar_svm::system_program::ID,
                executable: false,
            },
            Account {
                address: scoring_state,
                lamports: 0,
                data: vec![],
                owner: quasar_svm::system_program::ID,
                executable: false,
            },
        ],
    );

    result.assert_success();

    let scoring_account = result.account(&scoring_state).unwrap();
    // Discriminator byte should be 1
    assert_eq!(scoring_account.data[0], 1, "discriminator should be 1");
    // Phase byte (first field after discriminator) should be 0 (IDLE)
    assert_eq!(scoring_account.data[1], 0, "phase should be PHASE_IDLE (0)");
}
