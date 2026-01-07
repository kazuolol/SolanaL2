//! World Program State
//!
//! Account structures for the L2 game world.
//! Updated for 3D movement with physics.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

use crate::constants::*;

/// World configuration - singleton per world
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct WorldConfig {
    /// World name (max 32 bytes)
    pub name: [u8; 32],
    /// World admin authority
    pub authority: Pubkey,
    /// World dimensions (width in fixed-point units) - X axis
    pub width: u32,
    /// World dimensions (depth in fixed-point units) - Z axis
    pub depth: u32,
    /// Maximum players allowed
    pub max_players: u16,
    /// Current player count
    pub player_count: u16,
    /// Expected tick rate (for client reference)
    pub tick_rate: u8,
    /// PDA bump seed
    pub bump: u8,
    /// L1 Game PDA reference (for future integration)
    pub l1_game: Pubkey,
    /// Initialization timestamp
    pub init_ts: i64,
}

impl WorldConfig {
    /// Account size
    pub const LEN: usize = 32 + 32 + 4 + 4 + 2 + 2 + 1 + 1 + 32 + 8;

    /// Derive PDA for world config
    pub fn derive_pda(name: &[u8], program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[WORLD_SEED, name], program_id)
    }

    /// Get world name as string
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(&self.name)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Check if world is full
    pub fn is_full(&self) -> bool {
        self.player_count >= self.max_players
    }
}

/// Player state in the world (3D)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct WorldPlayer {
    /// Player wallet authority
    pub authority: Pubkey,
    /// World this player belongs to
    pub world: Pubkey,

    // 3D Position (X/Z ground plane, Y vertical for jumping)
    /// X position (fixed-point, 1000 = 1.0)
    pub position_x: i32,
    /// Z position (fixed-point, 1000 = 1.0) - ground plane horizontal
    pub position_z: i32,
    /// Y position (fixed-point, 1000 = 1.0) - vertical for jumping
    pub position_y: i32,

    // 3D Velocity
    /// X velocity
    pub velocity_x: i16,
    /// Z velocity
    pub velocity_z: i16,
    /// Y velocity (vertical - for jumping/falling)
    pub velocity_y: i16,

    /// Player yaw rotation (0-65535 maps to 0-360 degrees)
    pub yaw: i16,
    /// Current health
    pub health: u16,
    /// Maximum health
    pub max_health: u16,
    /// Last action slot (for rate limiting)
    pub last_action_slot: u64,
    /// Last combat timestamp (for cooldowns)
    pub last_combat_ts: i64,
    /// Is player in PVP zone (for future L1 sync)
    pub in_pvp_zone: bool,
    /// Is player on the ground
    pub is_grounded: bool,
    /// PDA bump seed
    pub bump: u8,
    /// Player name (max 16 bytes)
    pub name: [u8; 16],
}

impl WorldPlayer {
    /// Account size: 32 + 32 + 4 + 4 + 4 + 2 + 2 + 2 + 2 + 2 + 2 + 8 + 8 + 1 + 1 + 1 + 16 = 123
    pub const LEN: usize = 32 + 32 + 4 + 4 + 4 + 2 + 2 + 2 + 2 + 2 + 2 + 8 + 8 + 1 + 1 + 1 + 16;

    /// Derive PDA for world player
    pub fn derive_pda(world: &Pubkey, authority: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(
            &[WORLD_PLAYER_SEED, world.as_ref(), authority.as_ref()],
            program_id,
        )
    }

    /// Get player name as string
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(&self.name)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Check if player is alive
    pub fn is_alive(&self) -> bool {
        self.health > 0
    }

    /// Apply damage to player
    pub fn apply_damage(&mut self, damage: u16) {
        self.health = self.health.saturating_sub(damage);
    }

    /// Apply healing to player
    pub fn apply_heal(&mut self, heal: u16) {
        self.health = std::cmp::min(self.health.saturating_add(heal), self.max_health);
    }

    /// Apply 3D movement with physics
    pub fn apply_movement_3d(&mut self, input: &MovementInput3D, world: &WorldConfig) {
        // Convert camera-relative input to world-space direction
        let (world_dx, world_dz) = self.camera_to_world_direction(
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
        self.velocity_x = self.accelerate_toward(self.velocity_x, target_vx, ACCELERATION);
        self.velocity_z = self.accelerate_toward(self.velocity_z, target_vz, ACCELERATION);

        // Handle jumping
        if input.jump && self.is_grounded {
            self.velocity_y = JUMP_VELOCITY;
            self.is_grounded = false;
        }

        // Apply gravity if not grounded
        if !self.is_grounded {
            self.velocity_y = (self.velocity_y as i32 + GRAVITY as i32)
                .max(TERMINAL_VELOCITY as i32) as i16;
        }

        // Apply friction when no input and grounded
        if input.move_x == 0 && input.move_z == 0 && self.is_grounded {
            self.velocity_x = self.apply_friction(self.velocity_x, FRICTION);
            self.velocity_z = self.apply_friction(self.velocity_z, FRICTION);
        }

        // Update positions
        self.position_x = (self.position_x + self.velocity_x as i32)
            .clamp(0, (world.width as i32) * FIXED_POINT_SCALE);
        self.position_z = (self.position_z + self.velocity_z as i32)
            .clamp(0, (world.depth as i32) * FIXED_POINT_SCALE);
        self.position_y = (self.position_y + self.velocity_y as i32)
            .clamp(GROUND_LEVEL, MAX_HEIGHT);

        // Ground collision
        if self.position_y <= GROUND_LEVEL {
            self.position_y = GROUND_LEVEL;
            self.velocity_y = 0;
            self.is_grounded = true;
        }

        // Update yaw from camera
        self.yaw = input.camera_yaw;
    }

    /// Convert camera-relative movement to world-space direction
    fn camera_to_world_direction(&self, move_x: i8, move_z: i8, camera_yaw: i16) -> (i8, i8) {
        if move_x == 0 && move_z == 0 {
            return (0, 0);
        }

        // Convert camera yaw to radians
        // yaw: 0 = +Z (forward), 16384 = +X (right), 32768 = -Z (back), 49152 = -X (left)
        let yaw_rad = (camera_yaw as f32) * std::f32::consts::PI * 2.0 / 65536.0;
        let sin_yaw = yaw_rad.sin();
        let cos_yaw = yaw_rad.cos();

        // Rotate input by camera yaw
        // Forward (move_z positive) should go in camera direction
        // Right (move_x positive) should go perpendicular to camera
        let world_x = (move_x as f32 * cos_yaw + move_z as f32 * sin_yaw) as i8;
        let world_z = (-move_x as f32 * sin_yaw + move_z as f32 * cos_yaw) as i8;

        (world_x, world_z)
    }

    /// Accelerate toward target velocity
    fn accelerate_toward(&self, current: i16, target: i16, accel: i16) -> i16 {
        if current < target {
            (current as i32 + accel as i32).min(target as i32) as i16
        } else if current > target {
            (current as i32 - accel as i32).max(target as i32) as i16
        } else {
            current
        }
    }

    /// Apply friction to velocity
    fn apply_friction(&self, velocity: i16, friction: i16) -> i16 {
        if velocity > 0 {
            (velocity as i32 - friction as i32).max(0) as i16
        } else if velocity < 0 {
            (velocity as i32 + friction as i32).min(0) as i16
        } else {
            0
        }
    }

    /// Legacy 2D movement (for compatibility)
    pub fn apply_movement(&mut self, direction: u8, sprint: bool, world: &WorldConfig) {
        let (dx, dz) = direction_to_vector(direction);
        let speed = if sprint { SPRINT_SPEED } else { NORMAL_SPEED };

        self.velocity_x = (dx * speed as i32) as i16;
        self.velocity_z = (dz * speed as i32) as i16;

        if direction < 8 {
            // Convert direction to yaw (0-7 -> 0-65535)
            self.yaw = (direction as i16) * 8192;
        }

        // Apply velocity to position
        self.position_x += self.velocity_x as i32;
        self.position_z += self.velocity_z as i32;

        // Clamp to world bounds
        let max_x = (world.width as i32) * FIXED_POINT_SCALE;
        let max_z = (world.depth as i32) * FIXED_POINT_SCALE;

        self.position_x = self.position_x.clamp(0, max_x);
        self.position_z = self.position_z.clamp(0, max_z);
    }

    /// Calculate distance to another player (squared, to avoid sqrt)
    pub fn distance_squared(&self, other: &WorldPlayer) -> i64 {
        let dx = (self.position_x - other.position_x) as i64;
        let dz = (self.position_z - other.position_z) as i64;
        let dy = (self.position_y - other.position_y) as i64;
        dx * dx + dz * dz + dy * dy
    }
}

/// 3D Movement input from client
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, Default)]
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

/// Legacy movement input (for compatibility)
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug)]
pub struct MovementInput {
    /// Direction (0-7 for 8 directions, 255 = stop)
    pub direction: u8,
    /// Sprint modifier
    pub sprint: bool,
}

/// Weapon stats (placeholder for L1 integration)
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, Default)]
pub struct WeaponStats {
    pub damage: u16,
    pub range: u16,
    pub attack_speed: u8,
}

/// Convert direction (0-7) to unit vector (for legacy support)
pub fn direction_to_vector(direction: u8) -> (i32, i32) {
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
