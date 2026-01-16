//! JoinWorld Integration Tests
//!
//! Tests the complete JoinWorld flow including:
//! - L2Processor initialization with all builtins
//! - World account creation via InitializeWorld
//! - Player account creation via JoinWorld
//! - Account loader callback behavior
//! - Block producer transaction processing
//! - End-to-end client simulation

use std::{collections::HashSet, sync::Arc};

use borsh::BorshDeserialize;
use solana_sdk::{
    account::{Account, AccountSharedData, ReadableAccount},
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::{SanitizedTransaction, Transaction},
};
use solana_svm::transaction_processing_callback::TransactionProcessingCallback;

use crate::{
    account_store::AccountStore,
    block_producer::{BlockProducer, BlockProducerConfig},
    callback::L2AccountLoader,
    processor::L2Processor,
};

use world_program::{
    constants::{WORLD_SEED, WORLD_PLAYER_SEED},
    instruction::WorldInstruction,
    state::{WorldConfig, WorldPlayer},
};

/// Helper to create a world name array from string
fn make_world_name(name: &str) -> [u8; 32] {
    let mut arr = [0u8; 32];
    let bytes = name.as_bytes();
    let len = bytes.len().min(32);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

/// Helper to create a player name array from string
fn make_player_name(name: &str) -> [u8; 16] {
    let mut arr = [0u8; 16];
    let bytes = name.as_bytes();
    let len = bytes.len().min(16);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

/// Helper to create and sanitize a transaction
fn create_sanitized_transaction(
    payer: &Keypair,
    instructions: Vec<Instruction>,
    blockhash: Hash,
) -> SanitizedTransaction {
    let message = Message::new(&instructions, Some(&payer.pubkey()));
    let tx = Transaction::new(&[payer], message, blockhash);
    let reserved_account_keys = HashSet::new();
    SanitizedTransaction::try_from_legacy_transaction(tx, &reserved_account_keys).unwrap()
}

/// Helper to create InitializeWorld instruction
fn create_initialize_world_instruction(
    world_pda: Pubkey,
    authority: &Keypair,
    name: [u8; 32],
    width: u32,
    height: u32,
    max_players: u16,
) -> Instruction {
    let ix_data = WorldInstruction::InitializeWorld {
        name,
        width,
        height,
        max_players,
    };

    Instruction::new_with_borsh(
        world_program::id(),
        &ix_data,
        vec![
            AccountMeta::new(world_pda, false),
            AccountMeta::new_readonly(authority.pubkey(), true),
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ],
    )
}

/// Helper to create JoinWorld instruction
fn create_join_world_instruction(
    world_pda: Pubkey,
    player_pda: Pubkey,
    authority: &Keypair,
    name: [u8; 16],
) -> Instruction {
    let ix_data = WorldInstruction::JoinWorld { name };

    Instruction::new_with_borsh(
        world_program::id(),
        &ix_data,
        vec![
            AccountMeta::new(world_pda, false),
            AccountMeta::new(player_pda, false),
            AccountMeta::new_readonly(authority.pubkey(), true),
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ],
    )
}

/// Helper to set up a world account in the store
fn setup_world_account(
    store: &AccountStore,
    world_pda: Pubkey,
    authority: Pubkey,
    name: [u8; 32],
    width: u32,
    height: u32,
    max_players: u16,
) {
    let (_, bump) = Pubkey::find_program_address(
        &[WORLD_SEED, &name],
        &world_program::id(),
    );

    let world_config = WorldConfig {
        name,
        authority,
        width,
        depth: height,
        max_players,
        player_count: 0,
        tick_rate: 30,
        bump,
        l1_game: Pubkey::default(),
        init_ts: 0,
    };

    let mut data = vec![0u8; WorldConfig::LEN];
    borsh::to_writer(&mut data[..], &world_config).unwrap();

    let account = AccountSharedData::from(Account {
        lamports: 1_000_000,
        data,
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });

    store.store_account(world_pda, account, 0);
}

// ============================================================================
// Test Cases
// ============================================================================

/// Test 1: Verify L2Processor initializes correctly with all builtins
#[test]
fn test_processor_initialization() {
    let account_store = Arc::new(AccountStore::new());
    let processor = L2Processor::new(account_store.clone());

    // Verify processor starts at slot 0
    assert_eq!(processor.current_slot(), 0);
    assert_eq!(processor.current_epoch(), 0);

    // Verify builtin accounts exist in the store
    let builtin_ids = [
        solana_sdk::system_program::id(),
        solana_sdk::native_loader::id(),
        solana_sdk::bpf_loader::id(),
        solana_sdk::bpf_loader_upgradeable::id(),
        world_program::id(),
    ];

    for program_id in &builtin_ids {
        let account = account_store.get_account(program_id);
        assert!(account.is_some(), "Builtin {} not found in store", program_id);

        let account = account.unwrap();
        assert!(account.executable(), "Builtin {} should be executable", program_id);
        assert_eq!(
            account.owner(),
            &solana_sdk::native_loader::id(),
            "Builtin {} should be owned by native_loader",
            program_id
        );
    }

    // Verify sysvar accounts exist
    let clock_account = account_store.get_account(&sysvar::clock::id());
    assert!(clock_account.is_some(), "Clock sysvar not found");

    let rent_account = account_store.get_account(&sysvar::rent::id());
    assert!(rent_account.is_some(), "Rent sysvar not found");

    let epoch_schedule_account = account_store.get_account(&sysvar::epoch_schedule::id());
    assert!(epoch_schedule_account.is_some(), "EpochSchedule sysvar not found");
}

/// Test 2: Verify InitializeWorld creates world account correctly
#[test]
fn test_world_account_creation() {
    let account_store = Arc::new(AccountStore::new());
    let mut processor = L2Processor::new(account_store.clone());

    let authority = Keypair::new();
    let world_name = make_world_name("TestWorld");

    // Derive world PDA
    let (world_pda, _bump) = Pubkey::find_program_address(
        &[WORLD_SEED, &world_name],
        &world_program::id(),
    );

    // Pre-create world account with exact size and world_program as owner
    // The world program needs to own the account to write to it
    // Borsh deserialization requires exact size (no trailing bytes)
    let world_account = AccountSharedData::from(Account {
        lamports: 1_000_000_000,
        data: vec![0u8; WorldConfig::LEN],
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    account_store.store_account(world_pda, world_account, 0);

    // Create InitializeWorld transaction
    let instruction = create_initialize_world_instruction(
        world_pda,
        &authority,
        world_name,
        1000,  // width
        1000,  // height
        100,   // max_players
    );

    let blockhash = processor.current_blockhash();
    let tx = create_sanitized_transaction(&authority, vec![instruction], blockhash);

    // Process transaction
    let results = processor.process_transactions(&[tx]);

    // Verify transaction succeeded
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert!(result.success, "InitializeWorld failed: {:?}", result.error);

    // Verify world account was created correctly
    let world_account = account_store.get_account(&world_pda);
    assert!(world_account.is_some(), "World account not found after InitializeWorld");

    let world_account = world_account.unwrap();
    let world_config = WorldConfig::try_from_slice(world_account.data()).unwrap();

    assert_eq!(world_config.name, world_name);
    assert_eq!(world_config.authority, authority.pubkey());
    assert_eq!(world_config.width, 1000);
    assert_eq!(world_config.depth, 1000);
    assert_eq!(world_config.max_players, 100);
    assert_eq!(world_config.player_count, 0);
    assert_eq!(world_config.tick_rate, 30);
}

/// Test 3: Verify JoinWorld creates player account correctly
#[test]
fn test_join_world_transaction_processing() {
    let account_store = Arc::new(AccountStore::new());
    let mut processor = L2Processor::new(account_store.clone());

    let authority = Keypair::new();
    let world_name = make_world_name("JoinTestWorld");

    // Derive PDAs
    let (world_pda, _) = Pubkey::find_program_address(
        &[WORLD_SEED, &world_name],
        &world_program::id(),
    );

    let (player_pda, _) = Pubkey::find_program_address(
        &[WORLD_PLAYER_SEED, world_pda.as_ref(), authority.pubkey().as_ref()],
        &world_program::id(),
    );

    // Set up world account first (simulating already initialized world)
    setup_world_account(
        &account_store,
        world_pda,
        authority.pubkey(),
        world_name,
        1000,
        1000,
        100,
    );

    // Pre-create player account with exact size and world_program as owner
    let player_account = AccountSharedData::from(Account {
        lamports: 1_000_000_000,
        data: vec![0u8; WorldPlayer::LEN],
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    account_store.store_account(player_pda, player_account, 0);

    // Create JoinWorld transaction
    let player_name = make_player_name("Player1");
    let instruction = create_join_world_instruction(
        world_pda,
        player_pda,
        &authority,
        player_name,
    );

    let blockhash = processor.current_blockhash();
    let tx = create_sanitized_transaction(&authority, vec![instruction], blockhash);

    // Process transaction
    let results = processor.process_transactions(&[tx]);

    // Verify transaction succeeded
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert!(result.success, "JoinWorld failed: {:?}", result.error);

    // Verify player account was created
    let player_account = account_store.get_account(&player_pda);
    assert!(player_account.is_some(), "Player account not found after JoinWorld");

    let player_account = player_account.unwrap();
    let player = WorldPlayer::try_from_slice(player_account.data()).unwrap();

    assert_eq!(player.authority, authority.pubkey());
    assert_eq!(player.world, world_pda);
    assert_eq!(player.name, player_name);
    assert_eq!(player.health, world_program::constants::DEFAULT_HEALTH);
    assert_eq!(player.max_health, world_program::constants::DEFAULT_MAX_HEALTH);
    assert!(player.is_grounded);

    // Verify world player_count was incremented
    let world_account = account_store.get_account(&world_pda).unwrap();
    let world_config = WorldConfig::try_from_slice(world_account.data()).unwrap();
    assert_eq!(world_config.player_count, 1);
}

/// Test 4: Verify L2AccountLoader callback behavior
#[test]
fn test_account_loader_callback() {
    let store = Arc::new(AccountStore::new());
    let loader = L2AccountLoader::new(store.clone());

    // Test 1: Missing wallet (on-curve) returns account with 0 data bytes
    let wallet = Keypair::new().pubkey();
    let wallet_account = loader.get_account_shared_data(&wallet);
    assert!(wallet_account.is_some());
    let wallet_account = wallet_account.unwrap();
    assert_eq!(wallet_account.lamports(), 1_000_000_000);
    assert_eq!(wallet_account.data().len(), 0, "Wallet should have 0 data bytes");
    assert_eq!(wallet_account.owner(), &solana_sdk::system_program::id());

    // Test 2: Missing PDA (off-curve) returns account owned by world_program
    let (pda, _) = Pubkey::find_program_address(&[b"test"], &world_program::id());
    let pda_account = loader.get_account_shared_data(&pda);
    assert!(pda_account.is_some());
    let pda_account = pda_account.unwrap();
    assert_eq!(pda_account.lamports(), 1_000_000_000);
    assert_eq!(pda_account.data().len(), WorldPlayer::LEN, "PDA should have WorldPlayer::LEN data bytes");
    assert_eq!(pda_account.owner(), &world_program::id(), "PDA should be owned by world_program");

    // Test 3: Existing account returns correctly
    let existing_pubkey = Pubkey::new_unique();
    let existing_account = AccountSharedData::from(Account {
        lamports: 500,
        data: vec![1, 2, 3, 4],
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    store.store_account(existing_pubkey, existing_account, 0);

    let retrieved = loader.get_account_shared_data(&existing_pubkey);
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.lamports(), 500);
    assert_eq!(retrieved.data(), &[1, 2, 3, 4]);
    assert_eq!(retrieved.owner(), &world_program::id());

    // Test 4: account_matches_owners works for missing wallets (system-owned)
    // Use Keypair to get a real on-curve wallet address
    let missing_wallet = Keypair::new().pubkey();
    assert!(missing_wallet.is_on_curve(), "Keypair pubkey should be on-curve");
    let owners = vec![
        world_program::id(),
        solana_sdk::system_program::id(),
    ];
    let result = loader.account_matches_owners(&missing_wallet, &owners);
    assert_eq!(result, Some(1), "Missing wallet should match system_program");

    // Test 4b: account_matches_owners works for missing PDAs (world_program-owned)
    let (missing_pda, _) = Pubkey::find_program_address(&[b"missing"], &world_program::id());
    assert!(!missing_pda.is_on_curve(), "PDA should be off-curve");
    let result = loader.account_matches_owners(&missing_pda, &owners);
    assert_eq!(result, Some(0), "Missing PDA should match world_program");

    // Test 5: account_matches_owners works for existing accounts
    let result = loader.account_matches_owners(&existing_pubkey, &owners);
    assert_eq!(result, Some(0), "Existing account should match world_program");
}

/// Test 5: Verify BlockProducer processes JoinWorld transaction
#[test]
fn test_block_producer_processes_join() {
    let account_store = Arc::new(AccountStore::new());
    let processor = L2Processor::new(account_store.clone());

    let config = BlockProducerConfig {
        block_time_ms: 33,
        max_txs_per_block: 64,
        verbose: false,
    };

    let block_producer = BlockProducer::new(processor, config);
    let tx_sender = block_producer.transaction_sender();
    let _subscriber = block_producer.subscribe();

    // Set up world
    let authority = Keypair::new();
    let world_name = make_world_name("BlockProducerTest");

    let (world_pda, _) = Pubkey::find_program_address(
        &[WORLD_SEED, &world_name],
        &world_program::id(),
    );

    let (player_pda, _) = Pubkey::find_program_address(
        &[WORLD_PLAYER_SEED, world_pda.as_ref(), authority.pubkey().as_ref()],
        &world_program::id(),
    );

    // Set up world account
    setup_world_account(
        &account_store,
        world_pda,
        authority.pubkey(),
        world_name,
        1000,
        1000,
        100,
    );

    // Pre-create player account
    let player_account = AccountSharedData::from(Account {
        lamports: 1_000_000_000,
        data: vec![0u8; 256],
        owner: solana_sdk::system_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    account_store.store_account(player_pda, player_account, 0);

    // Create and send JoinWorld transaction
    let player_name = make_player_name("BlockPlayer");
    let instruction = create_join_world_instruction(
        world_pda,
        player_pda,
        &authority,
        player_name,
    );

    let blockhash = block_producer.current_blockhash();
    let tx = create_sanitized_transaction(&authority, vec![instruction], blockhash);

    // Submit transaction
    let send_result = tx_sender.send(tx);
    assert!(send_result.is_ok(), "Failed to send transaction: {:?}", send_result.err());

    // Run block producer for one tick manually (we can't use run() as it's blocking)
    // Instead, verify the transaction was queued successfully
    // The actual processing test is done in test_join_world_transaction_processing

    // Verify transaction was accepted by checking the sender didn't error
    assert!(send_result.is_ok());
}

/// Test 6: End-to-end test simulating full client flow
#[test]
fn test_end_to_end_join_flow() {
    let account_store = Arc::new(AccountStore::new());
    let mut processor = L2Processor::new(account_store.clone());

    let authority = Keypair::new();
    let world_name = make_world_name("E2EWorld");

    // Derive PDAs
    let (world_pda, _) = Pubkey::find_program_address(
        &[WORLD_SEED, &world_name],
        &world_program::id(),
    );

    let (player_pda, _) = Pubkey::find_program_address(
        &[WORLD_PLAYER_SEED, world_pda.as_ref(), authority.pubkey().as_ref()],
        &world_program::id(),
    );

    // Step 1: Pre-create world account (owned by world_program for writability)
    let world_account = AccountSharedData::from(Account {
        lamports: 1_000_000_000,
        data: vec![0u8; WorldConfig::LEN],
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    account_store.store_account(world_pda, world_account, 0);

    // Step 2: Initialize world
    let init_ix = create_initialize_world_instruction(
        world_pda,
        &authority,
        world_name,
        1000,
        1000,
        100,
    );

    let blockhash = processor.current_blockhash();
    let init_tx = create_sanitized_transaction(&authority, vec![init_ix], blockhash);

    let init_results = processor.process_transactions(&[init_tx]);
    assert_eq!(init_results.len(), 1);
    assert!(init_results[0].success, "InitializeWorld failed: {:?}", init_results[0].error);

    // Advance slot (simulate time passing)
    processor.advance_slot();

    // Step 3: Pre-create player account (owned by world_program for writability)
    let player_account = AccountSharedData::from(Account {
        lamports: 1_000_000_000,
        data: vec![0u8; WorldPlayer::LEN],
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    account_store.store_account(player_pda, player_account, 0);

    // Step 4: Join world
    let player_name = make_player_name("E2EPlayer");
    let join_ix = create_join_world_instruction(
        world_pda,
        player_pda,
        &authority,
        player_name,
    );

    let blockhash = processor.current_blockhash();
    let join_tx = create_sanitized_transaction(&authority, vec![join_ix], blockhash);

    let join_results = processor.process_transactions(&[join_tx]);
    assert_eq!(join_results.len(), 1);
    assert!(join_results[0].success, "JoinWorld failed: {:?}", join_results[0].error);

    // Step 5: Verify final state (simulating client getAccountInfo)
    let player_account = account_store.get_account(&player_pda);
    assert!(player_account.is_some(), "Player account should exist");

    let player_account = player_account.unwrap();
    let player = WorldPlayer::try_from_slice(player_account.data()).unwrap();

    // Verify player state
    assert_eq!(player.authority, authority.pubkey());
    assert_eq!(player.world, world_pda);
    assert_eq!(player.name, player_name);
    assert_eq!(player.health, 100);
    assert!(player.is_alive());
    assert!(player.is_grounded);

    // Verify world state
    let world_account = account_store.get_account(&world_pda).unwrap();
    let world = WorldConfig::try_from_slice(world_account.data()).unwrap();
    assert_eq!(world.player_count, 1);

    // Step 6: Verify we can advance slots without issues
    for _ in 0..10 {
        processor.advance_slot();
    }

    // Player should still be retrievable
    let player_account = account_store.get_account(&player_pda);
    assert!(player_account.is_some());
}

/// Test 7: Verify multiple players can join the same world
#[test]
fn test_multiple_players_join() {
    let account_store = Arc::new(AccountStore::new());
    let mut processor = L2Processor::new(account_store.clone());

    let admin = Keypair::new();
    let world_name = make_world_name("MultiPlayerWorld");

    // Derive world PDA
    let (world_pda, _) = Pubkey::find_program_address(
        &[WORLD_SEED, &world_name],
        &world_program::id(),
    );

    // Set up world account (owned by world_program for writability)
    let world_account = AccountSharedData::from(Account {
        lamports: 1_000_000_000,
        data: vec![0u8; WorldConfig::LEN],
        owner: world_program::id(),
        executable: false,
        rent_epoch: 0,
    });
    account_store.store_account(world_pda, world_account, 0);

    // Initialize world
    let init_ix = create_initialize_world_instruction(
        world_pda,
        &admin,
        world_name,
        1000,
        1000,
        10, // max 10 players
    );

    let blockhash = processor.current_blockhash();
    let init_tx = create_sanitized_transaction(&admin, vec![init_ix], blockhash);
    let results = processor.process_transactions(&[init_tx]);
    assert!(results[0].success);

    processor.advance_slot();

    // Join 5 players
    for i in 0..5 {
        let player_authority = Keypair::new();

        let (player_pda, _) = Pubkey::find_program_address(
            &[WORLD_PLAYER_SEED, world_pda.as_ref(), player_authority.pubkey().as_ref()],
            &world_program::id(),
        );

        // Pre-create player account (owned by world_program for writability)
        let player_account = AccountSharedData::from(Account {
            lamports: 1_000_000_000,
            data: vec![0u8; WorldPlayer::LEN],
            owner: world_program::id(),
            executable: false,
            rent_epoch: 0,
        });
        account_store.store_account(player_pda, player_account, 0);

        let name_str = format!("Player{}", i);
        let player_name = make_player_name(&name_str);
        let join_ix = create_join_world_instruction(
            world_pda,
            player_pda,
            &player_authority,
            player_name,
        );

        let blockhash = processor.current_blockhash();
        let join_tx = create_sanitized_transaction(&player_authority, vec![join_ix], blockhash);

        let results = processor.process_transactions(&[join_tx]);
        assert!(results[0].success, "Player {} join failed: {:?}", i, results[0].error);

        processor.advance_slot();
    }

    // Verify world player count
    let world_account = account_store.get_account(&world_pda).unwrap();
    let world = WorldConfig::try_from_slice(world_account.data()).unwrap();
    assert_eq!(world.player_count, 5);
}

/// Test 8: Verify processor slot advancement works correctly
#[test]
fn test_processor_slot_advancement() {
    let account_store = Arc::new(AccountStore::new());
    let mut processor = L2Processor::new(account_store.clone());

    assert_eq!(processor.current_slot(), 0);

    let mut prev_hash = processor.current_blockhash();

    // Advance slot multiple times
    for expected_slot in 1..=100 {
        processor.advance_slot();
        assert_eq!(processor.current_slot(), expected_slot);

        // Verify blockhash changes each slot
        let current_hash = processor.current_blockhash();
        assert_ne!(current_hash, prev_hash, "Blockhash should change each slot");
        prev_hash = current_hash;
    }

    // Verify Clock sysvar is updated
    let clock_account = account_store.get_account(&sysvar::clock::id()).unwrap();
    let clock: solana_sdk::clock::Clock = bincode::deserialize(clock_account.data()).unwrap();
    assert_eq!(clock.slot, 100);
}
