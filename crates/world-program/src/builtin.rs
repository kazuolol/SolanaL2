//! Native Builtin Entrypoint for World Program
//!
//! This module provides a native builtin wrapper that allows the world-program
//! to run as a native Rust builtin in the L2 SVM instead of as BPF bytecode.
//!
//! Benefits:
//! - Lower latency (no BPF interpreter)
//! - Easier debugging
//! - Full Rust standard library access

// Required by the declare_process_instruction! macro
use solana_sdk;

use borsh::BorshDeserialize;
use solana_program::instruction::InstructionError;
use solana_program_runtime::invoke_context::InvokeContext;

use crate::{
    constants::*,
    instruction::WorldInstruction,
    state::{MovementInput, MovementInput3D, WeaponStats, WorldConfig, WorldPlayer},
};

// Use the declare_process_instruction! macro to create a properly typed builtin entrypoint
// IMPORTANT: The macro creates a nested function called process_instruction_inner,
// so we must not call any function with that name from inside the macro body.
// Instead, we inline the processing code directly in the macro.
solana_program_runtime::declare_process_instruction!(Entrypoint, 200, |invoke_context| {
    eprintln!("[BUILTIN] Entry point called!");
    world_instruction_dispatch(invoke_context)
});

/// Dispatch world program instructions
/// NOTE: This function MUST have a different name than process_instruction_inner
/// because the declare_process_instruction! macro creates a nested function with that name.
fn world_instruction_dispatch(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
    eprintln!("[BUILTIN] world_instruction_dispatch ENTRY");
    solana_program::msg!("World program: process_instruction_inner called");
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Get instruction data
    let instruction_data = instruction_context.get_instruction_data();

    // Deserialize instruction
    eprintln!("[BUILTIN] deserializing instruction, data_len={}", instruction_data.len());
    let instruction = WorldInstruction::try_from_slice(instruction_data)
        .map_err(|_| InstructionError::InvalidInstructionData)?;
    eprintln!("[BUILTIN] instruction deserialized successfully");

    // Get program ID
    let program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(|_| InstructionError::UnsupportedProgramId)?;
    eprintln!("[BUILTIN] program_id for dispatch: {}", program_id);

    // Dispatch to instruction handler
    eprintln!("[BUILTIN] dispatching instruction...");
    match instruction {
        WorldInstruction::InitializeWorld {
            name,
            width,
            height,
            max_players,
        } => process_initialize_world(invoke_context, name, width, height, max_players),

        WorldInstruction::JoinWorld { name } => process_join_world(invoke_context, name),

        WorldInstruction::MovePlayer { input } => process_move_player(invoke_context, input),

        WorldInstruction::Attack { weapon_stats } => process_attack(invoke_context, weapon_stats),

        WorldInstruction::Heal { amount } => process_heal(invoke_context, amount),

        WorldInstruction::LeaveWorld => process_leave_world(invoke_context),

        WorldInstruction::UpdateWorld { max_players } => {
            process_update_world(invoke_context, max_players)
        }

        WorldInstruction::SetPvpZone { in_pvp_zone } => {
            process_set_pvp_zone(invoke_context, in_pvp_zone)
        }

        WorldInstruction::MovePlayer3D { input } => {
            process_move_player_3d(invoke_context, input)
        }
    }
}

/// Initialize a new world
fn process_initialize_world(
    invoke_context: &mut InvokeContext,
    name: [u8; 32],
    width: u32,
    height: u32,
    max_players: u16,
) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=authority, 2=payer, 3=system_program
    let mut world_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Get program ID for PDA derivation
    let program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(|_| InstructionError::UnsupportedProgramId)?;

    // Verify PDA
    let (expected_pda, bump) = WorldConfig::derive_pda(&name, program_id);
    if expected_pda != *world_account.get_key() {
        return Err(InstructionError::InvalidSeeds);
    }

    // Get clock for timestamp
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;

    // Initialize world config
    let world = WorldConfig {
        name,
        authority: *authority_account.get_key(),
        width,
        depth: height, // height parameter is now depth (Z axis)
        max_players,
        player_count: 0,
        tick_rate: 30,
        bump,
        l1_game: solana_program::pubkey::Pubkey::default(),
        init_ts: clock.unix_timestamp,
    };

    // Serialize to account data
    let data = world_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;

    if data.len() < WorldConfig::LEN {
        return Err(InstructionError::AccountDataTooSmall);
    }

    borsh::to_writer(&mut data[..], &world)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}

/// Join the world - create a new player
fn process_join_world(
    invoke_context: &mut InvokeContext,
    name: [u8; 16],
) -> Result<(), InstructionError> {
    eprintln!("[BUILTIN] process_join_world ENTRY");
    solana_program::msg!("World program: process_join_world called");
    let transaction_context = &*invoke_context.transaction_context;
    eprintln!("[BUILTIN] got transaction_context");
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;
    eprintln!("[BUILTIN] got instruction_context");

    // Account indices: 0=world, 1=player, 2=authority, 3=payer, 4=system_program
    eprintln!("[BUILTIN] about to borrow world_account (index 0)");
    let mut world_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] borrowed world_account successfully");

    eprintln!("[BUILTIN] about to borrow player_account (index 1)");
    let mut player_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] borrowed player_account successfully");

    eprintln!("[BUILTIN] about to borrow authority_account (index 2)");
    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] borrowed authority_account successfully");

    // Verify authority is signer
    eprintln!("[BUILTIN] checking authority is_signer");
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }
    eprintln!("[BUILTIN] authority is signer: OK");

    // Load world config
    eprintln!("[BUILTIN] about to get world_account.get_data()");
    let world_data = world_account.get_data();
    eprintln!("[BUILTIN] got world_data, len={}", world_data.len());
    let mut world = WorldConfig::try_from_slice(world_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] deserialized WorldConfig, player_count={}", world.player_count);

    // Check if world is full
    if world.is_full() {
        return Err(InstructionError::Custom(1)); // WorldFull
    }
    eprintln!("[BUILTIN] world not full: OK");

    // Get program ID for PDA derivation
    eprintln!("[BUILTIN] getting program_id");
    let program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(|_| InstructionError::UnsupportedProgramId)?;
    eprintln!("[BUILTIN] program_id = {}", program_id);

    // Verify player PDA
    eprintln!("[BUILTIN] deriving player PDA");
    let (expected_pda, bump) = WorldPlayer::derive_pda(
        world_account.get_key(),
        authority_account.get_key(),
        program_id,
    );
    eprintln!("[BUILTIN] expected_pda = {}, actual = {}", expected_pda, player_account.get_key());
    if expected_pda != *player_account.get_key() {
        return Err(InstructionError::InvalidSeeds);
    }
    eprintln!("[BUILTIN] PDA verified: OK");

    // Get clock for timestamp
    eprintln!("[BUILTIN] about to get clock from sysvar_cache");
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;
    eprintln!("[BUILTIN] got clock, slot={}", clock.slot);

    // Initialize player at world center
    let player = WorldPlayer {
        authority: *authority_account.get_key(),
        world: *world_account.get_key(),
        position_x: (world.width as i32 / 2) * FIXED_POINT_SCALE,
        position_z: (world.depth as i32 / 2) * FIXED_POINT_SCALE,
        position_y: 0, // Start on ground
        velocity_x: 0,
        velocity_z: 0,
        velocity_y: 0,
        yaw: 0,
        health: DEFAULT_HEALTH,
        max_health: DEFAULT_MAX_HEALTH,
        last_action_slot: clock.slot,
        last_combat_ts: 0,
        in_pvp_zone: false,
        is_grounded: true,
        bump,
        name,
    };

    // Serialize player to account data
    eprintln!("[BUILTIN] about to call player_account.get_data_mut() - THIS IS THE CRITICAL POINT");
    let player_data = player_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] got player_data_mut, len={}", player_data.len());

    if player_data.len() < WorldPlayer::LEN {
        eprintln!("[BUILTIN] ERROR: player_data.len()={} < WorldPlayer::LEN={}", player_data.len(), WorldPlayer::LEN);
        return Err(InstructionError::AccountDataTooSmall);
    }
    eprintln!("[BUILTIN] player data size OK, serializing player");

    borsh::to_writer(&mut player_data[..], &player)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] serialized player to account");

    // Update world player count
    world.player_count += 1;
    eprintln!("[BUILTIN] incremented player_count to {}", world.player_count);

    // Drop player_account borrow before mutating world_account again
    drop(player_account);
    eprintln!("[BUILTIN] dropped player_account borrow");

    eprintln!("[BUILTIN] about to call world_account.get_data_mut()");
    let world_data_mut = world_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] got world_data_mut, serializing world");
    borsh::to_writer(&mut world_data_mut[..], &world)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    eprintln!("[BUILTIN] serialized world to account");

    eprintln!("[BUILTIN] process_join_world SUCCESS");
    Ok(())
}

/// Move player
fn process_move_player(
    invoke_context: &mut InvokeContext,
    input: MovementInput,
) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=player, 2=authority
    let world_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let mut player_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Load world config
    let world_data = world_account.get_data();
    let world = WorldConfig::try_from_slice(world_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Load player
    let player_data = player_account.get_data();
    let mut player = WorldPlayer::try_from_slice(player_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority
    if player.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Verify world
    if player.world != *world_account.get_key() {
        return Err(InstructionError::Custom(3)); // InvalidWorld
    }

    // Check if alive
    if !player.is_alive() {
        return Err(InstructionError::Custom(4)); // PlayerDead
    }

    // Apply movement
    player.apply_movement(input.direction, input.sprint, &world);

    // Update last action slot
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;
    player.last_action_slot = clock.slot;

    // Serialize player back
    let player_data_mut = player_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut player_data_mut[..], &player)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}

/// Attack another player
fn process_attack(
    invoke_context: &mut InvokeContext,
    weapon_stats: Option<WeaponStats>,
) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=attacker, 2=target, 3=authority
    let mut attacker_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let mut target_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 3)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Cannot attack self
    if attacker_account.get_key() == target_account.get_key() {
        return Err(InstructionError::Custom(5)); // CannotAttackSelf
    }

    // Load attacker
    let attacker_data = attacker_account.get_data();
    let mut attacker = WorldPlayer::try_from_slice(attacker_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Load target
    let target_data = target_account.get_data();
    let mut target = WorldPlayer::try_from_slice(target_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify attacker authority
    if attacker.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Both must be alive
    if !attacker.is_alive() || !target.is_alive() {
        return Err(InstructionError::Custom(4)); // PlayerDead
    }

    // Calculate damage
    let damage = weapon_stats.map(|w| w.damage).unwrap_or(DEFAULT_DAMAGE);

    // Apply damage
    target.apply_damage(damage);

    // Update timestamps
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;
    attacker.last_combat_ts = clock.unix_timestamp;
    attacker.last_action_slot = clock.slot;

    // Save attacker
    let attacker_data_mut = attacker_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut attacker_data_mut[..], &attacker)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Save target
    let target_data_mut = target_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut target_data_mut[..], &target)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}

/// Heal self
fn process_heal(invoke_context: &mut InvokeContext, amount: u16) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=player, 2=authority
    let mut player_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Load player
    let player_data = player_account.get_data();
    let mut player = WorldPlayer::try_from_slice(player_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority
    if player.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Apply heal
    let heal_amount = if amount > 0 { amount } else { DEFAULT_HEAL };
    player.apply_heal(heal_amount);

    // Update last action slot
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;
    player.last_action_slot = clock.slot;

    // Save player
    let player_data_mut = player_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut player_data_mut[..], &player)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}

/// Leave the world
fn process_leave_world(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=player, 2=authority, 3=destination
    let mut world_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let player_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Load player
    let player_data = player_account.get_data();
    let player = WorldPlayer::try_from_slice(player_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority
    if player.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Load and update world
    let world_data = world_account.get_data();
    let mut world = WorldConfig::try_from_slice(world_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;
    world.player_count = world.player_count.saturating_sub(1);

    let world_data_mut = world_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut world_data_mut[..], &world)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Note: Account closing (lamport transfer) would be handled by system program
    // In builtin context, we just update the world player count

    Ok(())
}

/// Update world config
fn process_update_world(
    invoke_context: &mut InvokeContext,
    max_players: Option<u16>,
) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=authority
    let mut world_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Load world
    let world_data = world_account.get_data();
    let mut world = WorldConfig::try_from_slice(world_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority
    if world.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Update max players if provided
    if let Some(mp) = max_players {
        world.max_players = mp;
    }

    // Save world
    let world_data_mut = world_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut world_data_mut[..], &world)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}

/// Set player PVP zone status
fn process_set_pvp_zone(
    invoke_context: &mut InvokeContext,
    in_pvp_zone: bool,
) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=player, 1=authority
    let mut player_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Load player
    let player_data = player_account.get_data();
    let mut player = WorldPlayer::try_from_slice(player_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority
    if player.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Update PVP zone status
    player.in_pvp_zone = in_pvp_zone;

    // Save player
    let player_data_mut = player_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut player_data_mut[..], &player)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}

/// Move player with 3D physics
fn process_move_player_3d(
    invoke_context: &mut InvokeContext,
    input: MovementInput3D,
) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=player, 2=authority
    let world_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 0)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let mut player_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 1)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    let authority_account = instruction_context
        .try_borrow_instruction_account(transaction_context, 2)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority is signer
    if !authority_account.is_signer() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    // Load world config
    let world_data = world_account.get_data();
    let world = WorldConfig::try_from_slice(world_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Load player
    let player_data = player_account.get_data();
    let mut player = WorldPlayer::try_from_slice(player_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Verify authority
    if player.authority != *authority_account.get_key() {
        return Err(InstructionError::Custom(2)); // InvalidAuthority
    }

    // Verify world
    if player.world != *world_account.get_key() {
        return Err(InstructionError::Custom(3)); // InvalidWorld
    }

    // Check if alive
    if !player.is_alive() {
        return Err(InstructionError::Custom(4)); // PlayerDead
    }

    // Apply 3D movement with physics
    player.apply_movement_3d(&input, &world);

    // Update last action slot
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;
    player.last_action_slot = clock.slot;

    // Serialize player back
    let player_data_mut = player_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut player_data_mut[..], &player)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(())
}
