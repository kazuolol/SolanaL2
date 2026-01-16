/**
 * World Program State Types
 *
 * Matches the Rust structs in world-program/src/state.rs
 */

import { PublicKey } from '@solana/web3.js';

// Fixed-point scale: 1000 = 1.0 world unit
export const FIXED_POINT_SCALE = 1000;

// World program ID - must match the ID in world-program crate
export const WORLD_PROGRAM_ID = new PublicKey('Wor1dProgram1111111111111111111111111111111');

// Seeds
export const WORLD_SEED = Buffer.from('world');
export const WORLD_PLAYER_SEED = Buffer.from('world_player');

/** World configuration */
export interface WorldConfig {
  name: string;
  authority: PublicKey;
  width: number;
  depth: number; // Z axis (was height)
  maxPlayers: number;
  playerCount: number;
  tickRate: number;
  bump: number;
  l1Game: PublicKey;
  initTs: bigint;
}

/** Player state in the world (3D) */
export interface WorldPlayer {
  authority: PublicKey;
  world: PublicKey;
  // 3D Position (X/Z ground plane, Y vertical for jumping)
  positionX: number; // Fixed-point
  positionZ: number; // Fixed-point (ground plane)
  positionY: number; // Fixed-point (vertical - jumping)
  // 3D Velocity
  velocityX: number;
  velocityZ: number;
  velocityY: number; // Vertical velocity
  // Rotation
  yaw: number; // 0-65535 = 0-360 degrees
  // Combat
  health: number;
  maxHealth: number;
  lastActionSlot: bigint;
  lastCombatTs: bigint;
  // Flags
  inPvpZone: boolean;
  isGrounded: boolean;
  bump: number;
  name: string;
}

/** 3D Movement input */
export interface MovementInput3D {
  moveX: number;      // -127 to 127 (camera-relative left/right)
  moveZ: number;      // -127 to 127 (camera-relative forward/back)
  cameraYaw: number;  // 0-65535 (camera facing direction)
  sprint: boolean;
  jump: boolean;
}

/** Direction constants */
export enum Direction {
  North = 0,
  NorthEast = 1,
  East = 2,
  SouthEast = 3,
  South = 4,
  SouthWest = 5,
  West = 6,
  NorthWest = 7,
  Stop = 255,
}

/** Convert direction to vector */
export function directionToVector(direction: Direction): [number, number] {
  switch (direction) {
    case Direction.North: return [0, -1];
    case Direction.NorthEast: return [1, -1];
    case Direction.East: return [1, 0];
    case Direction.SouthEast: return [1, 1];
    case Direction.South: return [0, 1];
    case Direction.SouthWest: return [-1, 1];
    case Direction.West: return [-1, 0];
    case Direction.NorthWest: return [-1, -1];
    default: return [0, 0];
  }
}

/** Expected size of WorldPlayer account data */
export const WORLD_PLAYER_SIZE = 123;

/** Decode WorldPlayer from account data (3D layout) */
export function decodeWorldPlayer(data: Buffer): WorldPlayer {
  if (data.length < WORLD_PLAYER_SIZE) {
    throw new Error(`WorldPlayer data too short: ${data.length} bytes, expected at least ${WORLD_PLAYER_SIZE}`);
  }

  let offset = 0;

  // authority: Pubkey (32 bytes)
  const authority = new PublicKey(data.subarray(offset, offset + 32));
  offset += 32;

  // world: Pubkey (32 bytes)
  const world = new PublicKey(data.subarray(offset, offset + 32));
  offset += 32;

  // position_x: i32 (4 bytes)
  const positionX = data.readInt32LE(offset);
  offset += 4;

  // position_z: i32 (4 bytes) - ground plane Z
  const positionZ = data.readInt32LE(offset);
  offset += 4;

  // position_y: i32 (4 bytes) - vertical for jumping
  const positionY = data.readInt32LE(offset);
  offset += 4;

  // velocity_x: i16 (2 bytes)
  const velocityX = data.readInt16LE(offset);
  offset += 2;

  // velocity_z: i16 (2 bytes)
  const velocityZ = data.readInt16LE(offset);
  offset += 2;

  // velocity_y: i16 (2 bytes) - vertical velocity
  const velocityY = data.readInt16LE(offset);
  offset += 2;

  // yaw: i16 (2 bytes) - 0-65535 = 0-360 degrees
  const yaw = data.readInt16LE(offset);
  offset += 2;

  // health: u16 (2 bytes)
  const health = data.readUInt16LE(offset);
  offset += 2;

  // max_health: u16 (2 bytes)
  const maxHealth = data.readUInt16LE(offset);
  offset += 2;

  // last_action_slot: u64 (8 bytes)
  const lastActionSlot = data.readBigUInt64LE(offset);
  offset += 8;

  // last_combat_ts: i64 (8 bytes)
  const lastCombatTs = data.readBigInt64LE(offset);
  offset += 8;

  // in_pvp_zone: bool (1 byte)
  const inPvpZone = data.readUInt8(offset) !== 0;
  offset += 1;

  // is_grounded: bool (1 byte)
  const isGrounded = data.readUInt8(offset) !== 0;
  offset += 1;

  // bump: u8 (1 byte)
  const bump = data.readUInt8(offset);
  offset += 1;

  // name: [u8; 16] (16 bytes)
  const nameBytes = data.subarray(offset, offset + 16);
  const name = Buffer.from(nameBytes).toString('utf8').replace(/\0/g, '');

  return {
    authority,
    world,
    positionX,
    positionZ,
    positionY,
    velocityX,
    velocityZ,
    velocityY,
    yaw,
    health,
    maxHealth,
    lastActionSlot,
    lastCombatTs,
    inPvpZone,
    isGrounded,
    bump,
    name,
  };
}

/** Decode WorldConfig from account data */
export function decodeWorldConfig(data: Buffer): WorldConfig {
  let offset = 0;

  // name: [u8; 32] (32 bytes)
  const nameBytes = data.subarray(offset, offset + 32);
  const name = Buffer.from(nameBytes).toString('utf8').replace(/\0/g, '');
  offset += 32;

  // authority: Pubkey (32 bytes)
  const authority = new PublicKey(data.subarray(offset, offset + 32));
  offset += 32;

  // width: u32 (4 bytes)
  const width = data.readUInt32LE(offset);
  offset += 4;

  // depth: u32 (4 bytes) - Z axis
  const depth = data.readUInt32LE(offset);
  offset += 4;

  // max_players: u16 (2 bytes)
  const maxPlayers = data.readUInt16LE(offset);
  offset += 2;

  // player_count: u16 (2 bytes)
  const playerCount = data.readUInt16LE(offset);
  offset += 2;

  // tick_rate: u8 (1 byte)
  const tickRate = data.readUInt8(offset);
  offset += 1;

  // bump: u8 (1 byte)
  const bump = data.readUInt8(offset);
  offset += 1;

  // l1_game: Pubkey (32 bytes)
  const l1Game = new PublicKey(data.subarray(offset, offset + 32));
  offset += 32;

  // init_ts: i64 (8 bytes)
  const initTs = data.readBigInt64LE(offset);

  return {
    name,
    authority,
    width,
    depth,
    maxPlayers,
    playerCount,
    tickRate,
    bump,
    l1Game,
    initTs,
  };
}

/** Convert fixed-point position to world units */
export function toWorldUnits(fixedPoint: number): number {
  return fixedPoint / FIXED_POINT_SCALE;
}

/** Convert world units to fixed-point */
export function toFixedPoint(worldUnits: number): number {
  return Math.floor(worldUnits * FIXED_POINT_SCALE);
}

/** Derive WorldPlayer PDA */
export function deriveWorldPlayerPda(
  world: PublicKey,
  authority: PublicKey,
  programId: PublicKey = WORLD_PROGRAM_ID
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [WORLD_PLAYER_SEED, world.toBuffer(), authority.toBuffer()],
    programId
  );
}

/** Derive World PDA */
export function deriveWorldPda(
  name: string,
  programId: PublicKey = WORLD_PROGRAM_ID
): [PublicKey, number] {
  const nameBuffer = Buffer.alloc(32);
  nameBuffer.write(name);
  return PublicKey.findProgramAddressSync(
    [WORLD_SEED, nameBuffer],
    programId
  );
}
