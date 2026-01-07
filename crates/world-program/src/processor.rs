//! World Program Processor
//!
//! Handles instruction execution for the L2 game world.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

use crate::{
    constants::*,
    error::WorldError,
    instruction::WorldInstruction,
    state::{MovementInput3D, WorldConfig, WorldPlayer},
};

/// Process instruction
pub fn process(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = WorldInstruction::try_from_slice(instruction_data)
        .map_err(|_| WorldError::InvalidInstructionData)?;

    match instruction {
        WorldInstruction::InitializeWorld {
            name,
            width,
            height,
            max_players,
        } => process_initialize_world(program_id, accounts, name, width, height, max_players),

        WorldInstruction::JoinWorld { name } => process_join_world(program_id, accounts, name),

        WorldInstruction::MovePlayer { input } => process_move_player(program_id, accounts, input),

        WorldInstruction::Attack { weapon_stats } => {
            process_attack(program_id, accounts, weapon_stats)
        }

        WorldInstruction::Heal { amount } => process_heal(program_id, accounts, amount),

        WorldInstruction::LeaveWorld => process_leave_world(program_id, accounts),

        WorldInstruction::UpdateWorld { max_players } => {
            process_update_world(program_id, accounts, max_players)
        }

        WorldInstruction::SetPvpZone { in_pvp_zone } => {
            process_set_pvp_zone(program_id, accounts, in_pvp_zone)
        }

        WorldInstruction::MovePlayer3D { input } => {
            process_move_player_3d(program_id, accounts, input)
        }
    }
}

/// Initialize a new world
fn process_initialize_world(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    name: [u8; 32],
    width: u32,
    height: u32,
    max_players: u16,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let payer = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Derive PDA
    let (world_pda, bump) = WorldConfig::derive_pda(&name, program_id);
    if world_pda != *world_account.key {
        return Err(WorldError::InvalidWorld.into());
    }

    // Create account
    let rent = Rent::get()?;
    let space = WorldConfig::LEN;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            world_account.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[payer.clone(), world_account.clone(), system_program.clone()],
        &[&[WORLD_SEED, &name, &[bump]]],
    )?;

    // Initialize world config
    let clock = Clock::get()?;
    let world = WorldConfig {
        name,
        authority: *authority.key,
        width,
        depth: height, // height parameter is now depth (Z axis)
        max_players,
        player_count: 0,
        tick_rate: 30,
        bump,
        l1_game: Pubkey::default(),
        init_ts: clock.unix_timestamp,
    };

    world.serialize(&mut *world_account.data.borrow_mut())?;

    msg!("World initialized: {}", world.name_str());

    Ok(())
}

/// Join the world
fn process_join_world(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    name: [u8; 16],
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let player_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let payer = next_account_info(accounts_iter)?;
    let system_program = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Load world config
    let mut world = WorldConfig::try_from_slice(&world_account.data.borrow())?;

    // Check if world is full
    if world.is_full() {
        return Err(WorldError::WorldFull.into());
    }

    // Derive player PDA
    let (player_pda, bump) = WorldPlayer::derive_pda(world_account.key, authority.key, program_id);
    if player_pda != *player_account.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Create player account
    let rent = Rent::get()?;
    let space = WorldPlayer::LEN;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            player_account.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[
            payer.clone(),
            player_account.clone(),
            system_program.clone(),
        ],
        &[&[
            WORLD_PLAYER_SEED,
            world_account.key.as_ref(),
            authority.key.as_ref(),
            &[bump],
        ]],
    )?;

    // Initialize player at world center
    let clock = Clock::get()?;
    let player = WorldPlayer {
        authority: *authority.key,
        world: *world_account.key,
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

    player.serialize(&mut *player_account.data.borrow_mut())?;

    // Update world player count
    world.player_count += 1;
    world.serialize(&mut *world_account.data.borrow_mut())?;

    msg!("Player joined: {}", player.name_str());

    Ok(())
}

/// Move player
fn process_move_player(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: crate::state::MovementInput,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let player_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owners
    if player_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Load world and player
    let world = WorldConfig::try_from_slice(&world_account.data.borrow())?;
    let mut player = WorldPlayer::try_from_slice(&player_account.data.borrow())?;

    // Verify authority
    if player.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Verify world
    if player.world != *world_account.key {
        return Err(WorldError::InvalidWorld.into());
    }

    // Check if alive
    if !player.is_alive() {
        return Err(WorldError::PlayerDead.into());
    }

    // Apply movement
    player.apply_movement(input.direction, input.sprint, &world);

    // Update last action slot
    let clock = Clock::get()?;
    player.last_action_slot = clock.slot;

    // Save player
    player.serialize(&mut *player_account.data.borrow_mut())?;

    Ok(())
}

/// Attack another player
fn process_attack(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    weapon_stats: Option<crate::state::WeaponStats>,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let attacker_account = next_account_info(accounts_iter)?;
    let target_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owners
    if attacker_account.owner != program_id || target_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Cannot attack self
    if attacker_account.key == target_account.key {
        return Err(WorldError::CannotAttackSelf.into());
    }

    // Load players
    let mut attacker = WorldPlayer::try_from_slice(&attacker_account.data.borrow())?;
    let mut target = WorldPlayer::try_from_slice(&target_account.data.borrow())?;

    // Verify attacker authority
    if attacker.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Both must be alive
    if !attacker.is_alive() || !target.is_alive() {
        return Err(WorldError::PlayerDead.into());
    }

    // Calculate damage - use L1 stats if provided, else defaults
    let damage = weapon_stats
        .map(|w| w.damage)
        .unwrap_or(DEFAULT_DAMAGE);

    // Apply damage
    target.apply_damage(damage);

    // Update timestamps
    let clock = Clock::get()?;
    attacker.last_combat_ts = clock.unix_timestamp;
    attacker.last_action_slot = clock.slot;

    // Save both players
    attacker.serialize(&mut *attacker_account.data.borrow_mut())?;
    target.serialize(&mut *target_account.data.borrow_mut())?;

    msg!(
        "Attack: {} dealt {} damage to {}",
        attacker.name_str(),
        damage,
        target.name_str()
    );

    Ok(())
}

/// Heal self
fn process_heal(program_id: &Pubkey, accounts: &[AccountInfo], amount: u16) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let _world_account = next_account_info(accounts_iter)?;
    let player_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owner
    if player_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Load player
    let mut player = WorldPlayer::try_from_slice(&player_account.data.borrow())?;

    // Verify authority
    if player.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Use provided amount or default
    let heal_amount = if amount > 0 { amount } else { DEFAULT_HEAL };

    // Apply heal
    player.apply_heal(heal_amount);

    // Update last action slot
    let clock = Clock::get()?;
    player.last_action_slot = clock.slot;

    // Save player
    player.serialize(&mut *player_account.data.borrow_mut())?;

    msg!("Healed {} for {} HP", player.name_str(), heal_amount);

    Ok(())
}

/// Leave the world
fn process_leave_world(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let player_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;
    let destination = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owner
    if player_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Load player
    let player = WorldPlayer::try_from_slice(&player_account.data.borrow())?;

    // Verify authority
    if player.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Load and update world
    let mut world = WorldConfig::try_from_slice(&world_account.data.borrow())?;
    world.player_count = world.player_count.saturating_sub(1);
    world.serialize(&mut *world_account.data.borrow_mut())?;

    // Close player account (transfer lamports)
    let lamports = player_account.lamports();
    **player_account.lamports.borrow_mut() = 0;
    **destination.lamports.borrow_mut() += lamports;

    msg!("Player left: {}", player.name_str());

    Ok(())
}

/// Update world config
fn process_update_world(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    max_players: Option<u16>,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owner
    if world_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Load world
    let mut world = WorldConfig::try_from_slice(&world_account.data.borrow())?;

    // Verify authority
    if world.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Update max players if provided
    if let Some(mp) = max_players {
        world.max_players = mp;
    }

    // Save world
    world.serialize(&mut *world_account.data.borrow_mut())?;

    Ok(())
}

/// Set player PVP zone status
fn process_set_pvp_zone(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    in_pvp_zone: bool,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let player_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owner
    if player_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Load player
    let mut player = WorldPlayer::try_from_slice(&player_account.data.borrow())?;

    // Verify authority
    if player.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Update PVP zone status
    player.in_pvp_zone = in_pvp_zone;

    // Save player
    player.serialize(&mut *player_account.data.borrow_mut())?;

    Ok(())
}

/// Move player with 3D physics
fn process_move_player_3d(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: MovementInput3D,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let world_account = next_account_info(accounts_iter)?;
    let player_account = next_account_info(accounts_iter)?;
    let authority = next_account_info(accounts_iter)?;

    // Verify authority is signer
    if !authority.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify account owner
    if player_account.owner != program_id {
        return Err(WorldError::InvalidAccountOwner.into());
    }

    // Load world config
    let world = WorldConfig::try_from_slice(&world_account.data.borrow())?;

    // Load player
    let mut player = WorldPlayer::try_from_slice(&player_account.data.borrow())?;

    // Verify authority
    if player.authority != *authority.key {
        return Err(WorldError::InvalidAuthority.into());
    }

    // Verify world
    if player.world != *world_account.key {
        return Err(WorldError::InvalidWorld.into());
    }

    // Check if alive
    if !player.is_alive() {
        return Err(WorldError::PlayerDead.into());
    }

    // Apply 3D movement with physics
    player.apply_movement_3d(&input, &world);

    // Update last action slot
    let clock = Clock::get()?;
    player.last_action_slot = clock.slot;

    // Save player
    player.serialize(&mut *player_account.data.borrow_mut())?;

    Ok(())
}
