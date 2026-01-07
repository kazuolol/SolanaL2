//! Simple Game Handler for MVP
//!
//! Bypasses full SVM transaction processing for quick prototype.
//! Directly manipulates game state in the account store.
//! Updated for 3D movement with physics.
//! Now includes leader broadcast for validator network.

use borsh::{BorshDeserialize, BorshSerialize};
use l2_consensus::LeaderNode;
use l2_runtime::AccountStore;
use solana_sdk::{
    account::{Account, AccountSharedData, ReadableAccount},
    clock::Slot,
    pubkey::Pubkey,
};
use std::sync::Arc;

/// World player state (matches world-program state.rs - 3D version)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct WorldPlayer {
    pub authority: Pubkey,
    pub world: Pubkey,
    // 3D Position (X/Z ground plane, Y vertical for jumping)
    pub position_x: i32,
    pub position_z: i32,
    pub position_y: i32,
    // 3D Velocity
    pub velocity_x: i16,
    pub velocity_z: i16,
    pub velocity_y: i16,
    // Rotation
    pub yaw: i16,
    pub health: u16,
    pub max_health: u16,
    pub last_action_slot: u64,
    pub last_combat_ts: i64,
    pub in_pvp_zone: bool,
    pub is_grounded: bool,
    pub bump: u8,
    pub name: [u8; 16],
}

impl WorldPlayer {
    pub const LEN: usize = 32 + 32 + 4 + 4 + 4 + 2 + 2 + 2 + 2 + 2 + 2 + 8 + 8 + 1 + 1 + 1 + 16;
}

/// World config (matches world-program state.rs - 3D version)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct WorldConfig {
    pub name: [u8; 32],
    pub authority: Pubkey,
    pub width: u32,
    pub depth: u32, // Was height, now Z axis
    pub max_players: u16,
    pub player_count: u16,
    pub tick_rate: u8,
    pub bump: u8,
    pub l1_game: Pubkey,
    pub init_ts: i64,
}

impl WorldConfig {
    pub const LEN: usize = 32 + 32 + 4 + 4 + 2 + 2 + 1 + 1 + 32 + 8;
}

/// 3D Movement input from client
#[derive(Clone, Copy, Debug, Default)]
pub struct MovementInput3D {
    /// X movement (-127 to 127, camera-relative left/right)
    pub move_x: i8,
    /// Z movement (-127 to 127, camera-relative forward/back)
    pub move_z: i8,
    /// Camera yaw (0-65535 maps to 0-360 degrees)
    pub camera_yaw: i16,
    /// Sprint modifier
    pub sprint: bool,
    /// Jump input
    pub jump: bool,
}

/// Game constants
const FIXED_POINT_SCALE: i32 = 1000;
const NORMAL_SPEED: i16 = 250;
const SPRINT_SPEED: i16 = 500;
const ACCELERATION: i16 = 100;
const FRICTION: i16 = 50;
const GRAVITY: i16 = -30;
const JUMP_VELOCITY: i16 = 400;
const TERMINAL_VELOCITY: i16 = -800;
const GROUND_LEVEL: i32 = 0;
const MAX_HEIGHT: i32 = 50_000;
const DEFAULT_HEALTH: u16 = 100;
const DEFAULT_WORLD_WIDTH: u32 = 100;
const DEFAULT_WORLD_DEPTH: u32 = 100;

/// World program ID - must match world-program crate
pub fn world_program_id() -> Pubkey {
    // "Wor1dProgram11111111111111111111111111111111" in base58
    world_program::id()
}

/// Seeds for PDAs
const WORLD_SEED: &[u8] = b"world";
const WORLD_PLAYER_SEED: &[u8] = b"world_player";

/// Derive world PDA
pub fn derive_world_pda(name: &[u8]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[WORLD_SEED, name], &world_program_id())
}

/// Derive player PDA
pub fn derive_player_pda(world: &Pubkey, authority: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[WORLD_PLAYER_SEED, world.as_ref(), authority.as_ref()],
        &world_program_id(),
    )
}

/// Simple game handler with leader broadcast
pub struct GameHandler {
    account_store: Arc<AccountStore>,
    /// Leader node for broadcasting state changes (None if in validator mode)
    leader: Option<Arc<LeaderNode>>,
}

impl GameHandler {
    pub fn new(account_store: Arc<AccountStore>) -> Self {
        Self {
            account_store,
            leader: None,
        }
    }

    /// Create with leader node for broadcasting
    pub fn with_leader(account_store: Arc<AccountStore>, leader: Arc<LeaderNode>) -> Self {
        Self {
            account_store,
            leader: Some(leader),
        }
    }

    /// Record a write to the leader for broadcast
    fn record_write(&self, pubkey: Pubkey, account: &AccountSharedData) {
        if let Some(ref leader) = self.leader {
            leader.record_write(
                pubkey,
                account.data().to_vec(),
                account.lamports(),
                *account.owner(),
            );
        }
    }

    /// Store account and broadcast
    fn store_and_broadcast(&self, pubkey: Pubkey, account: AccountSharedData, slot: Slot) {
        self.account_store.store_account(pubkey, account.clone(), slot);
        self.record_write(pubkey, &account);
    }

    /// Initialize default world if it doesn't exist
    pub fn ensure_default_world(&self, slot: Slot) -> Pubkey {
        let mut name_bytes = [0u8; 32];
        name_bytes[..7].copy_from_slice(b"default");

        let (world_pda, bump) = derive_world_pda(&name_bytes);

        // Check if world exists
        if self.account_store.get_account(&world_pda).is_none() {
            let world_config = WorldConfig {
                name: name_bytes,
                authority: Pubkey::default(),
                width: DEFAULT_WORLD_WIDTH,
                depth: DEFAULT_WORLD_DEPTH,
                max_players: 100,
                player_count: 0,
                tick_rate: 30,
                bump,
                l1_game: Pubkey::default(),
                init_ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
            };

            let data = borsh::to_vec(&world_config).unwrap();
            let account = AccountSharedData::from(Account {
                lamports: 1,
                data,
                owner: world_program_id(),
                executable: false,
                rent_epoch: 0,
            });

            self.store_and_broadcast(world_pda, account, slot);
            tracing::info!("Created default world: {}", world_pda);
        }

        world_pda
    }

    /// Join world - create player account
    pub fn join_world(
        &self,
        authority: Pubkey,
        player_name: &str,
        slot: Slot,
    ) -> Result<Pubkey, String> {
        let world_pda = self.ensure_default_world(slot);
        let (player_pda, bump) = derive_player_pda(&world_pda, &authority);

        // Check if player already exists
        if self.account_store.get_account(&player_pda).is_some() {
            return Ok(player_pda); // Already joined
        }

        // Create player
        let mut name_bytes = [0u8; 16];
        let name_len = player_name.len().min(16);
        name_bytes[..name_len].copy_from_slice(&player_name.as_bytes()[..name_len]);

        // Random spawn position on ground plane
        let spawn_x = (rand::random::<u32>() % DEFAULT_WORLD_WIDTH) as i32 * FIXED_POINT_SCALE;
        let spawn_z = (rand::random::<u32>() % DEFAULT_WORLD_DEPTH) as i32 * FIXED_POINT_SCALE;

        let player = WorldPlayer {
            authority,
            world: world_pda,
            position_x: spawn_x,
            position_z: spawn_z,
            position_y: 0, // Start on ground
            velocity_x: 0,
            velocity_z: 0,
            velocity_y: 0,
            yaw: 0,
            health: DEFAULT_HEALTH,
            max_health: DEFAULT_HEALTH,
            last_action_slot: slot,
            last_combat_ts: 0,
            in_pvp_zone: false,
            is_grounded: true,
            bump,
            name: name_bytes,
        };

        let data = borsh::to_vec(&player).unwrap();
        let account = AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: world_program_id(),
            executable: false,
            rent_epoch: 0,
        });

        self.store_and_broadcast(player_pda, account, slot);

        // Update world player count
        if let Some(world_account) = self.account_store.get_account(&world_pda) {
            if let Ok(mut world_config) = WorldConfig::try_from_slice(world_account.data()) {
                world_config.player_count += 1;
                let data = borsh::to_vec(&world_config).unwrap();
                let updated = AccountSharedData::from(Account {
                    lamports: 1,
                    data,
                    owner: world_program_id(),
                    executable: false,
                    rent_epoch: 0,
                });
                self.store_and_broadcast(world_pda, updated, slot);
            }
        }

        tracing::info!(
            "Player {} joined world at ({}, {}, {})",
            player_pda,
            spawn_x / FIXED_POINT_SCALE,
            spawn_z / FIXED_POINT_SCALE,
            0
        );
        Ok(player_pda)
    }

    /// Move player with 3D physics
    pub fn move_player_3d(
        &self,
        authority: Pubkey,
        input: MovementInput3D,
        slot: Slot,
    ) -> Result<Pubkey, String> {
        let world_pda = self.ensure_default_world(slot);
        let (player_pda, _) = derive_player_pda(&world_pda, &authority);

        let player_account = self.account_store
            .get_account(&player_pda)
            .ok_or_else(|| "Player not found - join world first".to_string())?;

        use solana_sdk::account::ReadableAccount;
        let mut player = WorldPlayer::try_from_slice(player_account.data())
            .map_err(|e| format!("Failed to decode player: {}", e))?;

        // Convert camera-relative input to world-space direction
        let (world_dx, world_dz) = camera_to_world_direction(
            input.move_x,
            input.move_z,
            input.camera_yaw,
        );

        // Target velocity based on input
        let speed = if input.sprint { SPRINT_SPEED } else { NORMAL_SPEED };
        let target_vx = if world_dx != 0 {
            (world_dx as i32 * speed as i32 / 127) as i16
        } else {
            0
        };
        let target_vz = if world_dz != 0 {
            (world_dz as i32 * speed as i32 / 127) as i16
        } else {
            0
        };

        // Apply acceleration toward target velocity
        player.velocity_x = accelerate_toward(player.velocity_x, target_vx, ACCELERATION);
        player.velocity_z = accelerate_toward(player.velocity_z, target_vz, ACCELERATION);

        // Handle jumping
        if input.jump && player.is_grounded {
            player.velocity_y = JUMP_VELOCITY;
            player.is_grounded = false;
        }

        // Apply gravity if not grounded
        if !player.is_grounded {
            player.velocity_y = (player.velocity_y as i32 + GRAVITY as i32)
                .max(TERMINAL_VELOCITY as i32) as i16;
        }

        // Apply friction when no input and grounded
        if input.move_x == 0 && input.move_z == 0 && player.is_grounded {
            player.velocity_x = apply_friction(player.velocity_x, FRICTION);
            player.velocity_z = apply_friction(player.velocity_z, FRICTION);
        }

        // Update positions
        let max_x = (DEFAULT_WORLD_WIDTH as i32) * FIXED_POINT_SCALE;
        let max_z = (DEFAULT_WORLD_DEPTH as i32) * FIXED_POINT_SCALE;

        player.position_x = (player.position_x + player.velocity_x as i32).clamp(0, max_x);
        player.position_z = (player.position_z + player.velocity_z as i32).clamp(0, max_z);
        player.position_y = (player.position_y + player.velocity_y as i32).clamp(GROUND_LEVEL, MAX_HEIGHT);

        // Ground collision
        if player.position_y <= GROUND_LEVEL {
            player.position_y = GROUND_LEVEL;
            player.velocity_y = 0;
            player.is_grounded = true;
        }

        // Update yaw from camera
        player.yaw = input.camera_yaw;
        player.last_action_slot = slot;

        tracing::debug!(
            "Move3D: pos=({:.1}, {:.1}, {:.1}) vel=({}, {}, {}) grounded={}",
            player.position_x as f32 / FIXED_POINT_SCALE as f32,
            player.position_z as f32 / FIXED_POINT_SCALE as f32,
            player.position_y as f32 / FIXED_POINT_SCALE as f32,
            player.velocity_x,
            player.velocity_z,
            player.velocity_y,
            player.is_grounded
        );

        // Save updated player
        let data = borsh::to_vec(&player).unwrap();
        let account = AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: world_program_id(),
            executable: false,
            rent_epoch: 0,
        });

        self.store_and_broadcast(player_pda, account, slot);

        Ok(player_pda)
    }

    /// Legacy move player (2D, for compatibility)
    pub fn move_player(
        &self,
        authority: Pubkey,
        direction: u8,
        sprint: bool,
        slot: Slot,
    ) -> Result<Pubkey, String> {
        let world_pda = self.ensure_default_world(slot);
        let (player_pda, _) = derive_player_pda(&world_pda, &authority);

        let player_account = self.account_store
            .get_account(&player_pda)
            .ok_or_else(|| "Player not found - join world first".to_string())?;

        use solana_sdk::account::ReadableAccount;
        let mut player = WorldPlayer::try_from_slice(player_account.data())
            .map_err(|e| format!("Failed to decode player: {}", e))?;

        // Calculate movement
        let (dx, dz) = direction_to_vector(direction);
        let speed = if sprint { SPRINT_SPEED } else { NORMAL_SPEED };

        player.velocity_x = (dx * speed as i32) as i16;
        player.velocity_z = (dz * speed as i32) as i16;

        if direction < 8 {
            // Convert direction to yaw (0-7 -> 0-65535)
            player.yaw = (direction as i16) * 8192;
        }

        // Apply velocity
        player.position_x += player.velocity_x as i32;
        player.position_z += player.velocity_z as i32;

        // Clamp to world bounds
        let max_x = (DEFAULT_WORLD_WIDTH as i32) * FIXED_POINT_SCALE;
        let max_z = (DEFAULT_WORLD_DEPTH as i32) * FIXED_POINT_SCALE;
        player.position_x = player.position_x.clamp(0, max_x);
        player.position_z = player.position_z.clamp(0, max_z);

        player.last_action_slot = slot;

        // Save updated player
        let data = borsh::to_vec(&player).unwrap();
        let account = AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: world_program_id(),
            executable: false,
            rent_epoch: 0,
        });

        self.store_and_broadcast(player_pda, account, slot);

        Ok(player_pda)
    }

    /// Get player state
    pub fn get_player(&self, authority: &Pubkey) -> Option<WorldPlayer> {
        let mut name_bytes = [0u8; 32];
        name_bytes[..7].copy_from_slice(b"default");
        let (world_pda, _) = derive_world_pda(&name_bytes);
        let (player_pda, _) = derive_player_pda(&world_pda, authority);

        let account = self.account_store.get_account(&player_pda)?;
        use solana_sdk::account::ReadableAccount;
        WorldPlayer::try_from_slice(account.data()).ok()
    }

    /// Get all players in the world
    pub fn get_all_players(&self) -> Vec<(Pubkey, WorldPlayer)> {
        use solana_sdk::account::ReadableAccount;

        let mut players = Vec::new();

        // Get all accounts owned by world program
        for (pubkey, account) in self.account_store.get_program_accounts(&world_program_id()) {
            // Check if correct size for player account
            if account.data().len() == WorldPlayer::LEN {
                if let Ok(player) = WorldPlayer::try_from_slice(account.data()) {
                    // Verify it's a player account (has valid authority)
                    if player.authority != Pubkey::default() {
                        players.push((pubkey, player));
                    }
                }
            }
        }

        players
    }
}

/// Convert camera-relative movement to world-space direction
fn camera_to_world_direction(move_x: i8, move_z: i8, camera_yaw: i16) -> (i8, i8) {
    if move_x == 0 && move_z == 0 {
        return (0, 0);
    }

    // Convert camera yaw to radians
    let yaw_rad = (camera_yaw as f32) * std::f32::consts::PI * 2.0 / 65536.0;
    let sin_yaw = yaw_rad.sin();
    let cos_yaw = yaw_rad.cos();

    // Rotate input by camera yaw
    let world_x = (move_x as f32 * cos_yaw + move_z as f32 * sin_yaw) as i8;
    let world_z = (-move_x as f32 * sin_yaw + move_z as f32 * cos_yaw) as i8;

    (world_x, world_z)
}

/// Accelerate toward target velocity
fn accelerate_toward(current: i16, target: i16, accel: i16) -> i16 {
    if current < target {
        (current as i32 + accel as i32).min(target as i32) as i16
    } else if current > target {
        (current as i32 - accel as i32).max(target as i32) as i16
    } else {
        current
    }
}

/// Apply friction to velocity
fn apply_friction(velocity: i16, friction: i16) -> i16 {
    if velocity > 0 {
        (velocity as i32 - friction as i32).max(0) as i16
    } else if velocity < 0 {
        (velocity as i32 + friction as i32).min(0) as i16
    } else {
        0
    }
}

/// Convert direction (0-7) to unit vector (for legacy support)
fn direction_to_vector(direction: u8) -> (i32, i32) {
    match direction {
        0 => (0, -1),   // North (-Z)
        1 => (1, -1),   // Northeast
        2 => (1, 0),    // East (+X)
        3 => (1, 1),    // Southeast
        4 => (0, 1),    // South (+Z)
        5 => (-1, 1),   // Southwest
        6 => (-1, 0),   // West (-X)
        7 => (-1, -1),  // Northwest
        _ => (0, 0),    // Stop
    }
}
