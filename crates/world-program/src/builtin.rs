//! Native Builtin Entrypoint for World Program
//!
//! This module provides a native builtin wrapper that allows the world-program
//! to run as a native Rust builtin in the L2 SVM instead of as BPF bytecode.
//!
//! Benefits:
//! - Lower latency (no BPF interpreter)
//! - Easier debugging
//! - Full Rust standard library access

use borsh::BorshDeserialize;
use solana_program::instruction::InstructionError;
use solana_program_runtime::invoke_context::InvokeContext;

use crate::{
    constants::*,
    instruction::WorldInstruction,
    state::{MovementInput, WeaponStats, WorldConfig, WorldPlayer},
};

/// Builtin entrypoint for SVM registration
pub struct Entrypoint;

impl Entrypoint {
    /// VM entrypoint - this is registered with the SVM as a builtin
    pub fn vm(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
        process_instruction(invoke_context)
    }
}

/// Process a world program instruction
fn process_instruction(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Get instruction data
    let instruction_data = instruction_context.get_instruction_data();

    // Deserialize instruction
    let instruction = WorldInstruction::try_from_slice(instruction_data)
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Get program ID
    let program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(|_| InstructionError::UnsupportedProgramId)?;

    // Dispatch to instruction handler
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
    let transaction_context = &*invoke_context.transaction_context;
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .map_err(|_| InstructionError::InvalidInstructionData)?;

    // Account indices: 0=world, 1=player, 2=authority, 3=payer, 4=system_program
    let mut world_account = instruction_context
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
    let mut world = WorldConfig::try_from_slice(world_data)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Check if world is full
    if world.is_full() {
        return Err(InstructionError::Custom(1)); // WorldFull
    }

    // Get program ID for PDA derivation
    let program_id = instruction_context
        .get_last_program_key(transaction_context)
        .map_err(|_| InstructionError::UnsupportedProgramId)?;

    // Verify player PDA
    let (expected_pda, bump) = WorldPlayer::derive_pda(
        world_account.get_key(),
        authority_account.get_key(),
        program_id,
    );
    if expected_pda != *player_account.get_key() {
        return Err(InstructionError::InvalidSeeds);
    }

    // Get clock for timestamp
    let clock = invoke_context.get_sysvar_cache().get_clock()
        .map_err(|_| InstructionError::UnsupportedSysvar)?;

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
    let player_data = player_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;

    if player_data.len() < WorldPlayer::LEN {
        return Err(InstructionError::AccountDataTooSmall);
    }

    borsh::to_writer(&mut player_data[..], &player)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    // Update world player count
    world.player_count += 1;

    // Drop player_account borrow before mutating world_account again
    drop(player_account);

    let world_data_mut = world_account.get_data_mut()
        .map_err(|_| InstructionError::InvalidAccountData)?;
    borsh::to_writer(&mut world_data_mut[..], &world)
        .map_err(|_| InstructionError::InvalidAccountData)?;

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
