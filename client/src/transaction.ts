/**
 * Transaction Builder for World Program
 */

import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
} from '@solana/web3.js';
import { Direction, WORLD_PROGRAM_ID, deriveWorldPda, deriveWorldPlayerPda } from './state';

/** Instruction discriminants (matches WorldInstruction enum in Borsh order) */
enum WorldInstructionType {
  InitializeWorld = 0,
  JoinWorld = 1,
  MovePlayer = 2,
  Attack = 3,
  Heal = 4,
  LeaveWorld = 5,
  UpdateWorld = 6,
  SetPvpZone = 7,
  MovePlayer3D = 8,
}

/** Build JoinWorld instruction */
export function buildJoinWorldInstruction(
  world: PublicKey,
  player: PublicKey,
  authority: PublicKey,
  payer: PublicKey,
  playerName: string,
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data: [discriminant (1 byte), name (16 bytes)]
  // Borsh enum uses 1-byte (u8) discriminant
  const data = Buffer.alloc(1 + 16);
  data.writeUInt8(WorldInstructionType.JoinWorld, 0);

  const nameBuffer = Buffer.alloc(16);
  nameBuffer.write(playerName.slice(0, 16));
  nameBuffer.copy(data, 1);

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: true },
      { pubkey: player, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    programId,
    data,
  });
}

/** Build MovePlayer instruction */
export function buildMovePlayerInstruction(
  world: PublicKey,
  player: PublicKey,
  authority: PublicKey,
  direction: Direction,
  sprint: boolean = false,
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data: [discriminant (1 byte), direction (1 byte), sprint (1 byte)]
  const data = Buffer.alloc(1 + 2);
  data.writeUInt8(WorldInstructionType.MovePlayer, 0);
  data.writeUInt8(direction, 1);
  data.writeUInt8(sprint ? 1 : 0, 2);

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: false },
      { pubkey: player, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    programId,
    data,
  });
}

/** 3D Movement input */
export interface MovementInput3D {
  moveX: number;    // -127 to 127 (camera-relative left/right)
  moveZ: number;    // -127 to 127 (camera-relative forward/back)
  cameraYaw: number; // 0-65535 (0-360 degrees)
  sprint: boolean;
  jump: boolean;
}

/** Build MovePlayer3D instruction */
export function buildMovePlayer3DInstruction(
  world: PublicKey,
  player: PublicKey,
  authority: PublicKey,
  input: MovementInput3D,
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data layout (Borsh):
  // [discriminant (1 byte), move_x (i8), move_z (i8), camera_yaw (i16 LE), sprint (bool), jump (bool)]
  const data = Buffer.alloc(1 + 6);
  data.writeUInt8(WorldInstructionType.MovePlayer3D, 0);
  data.writeInt8(input.moveX, 1);
  data.writeInt8(input.moveZ, 2);
  data.writeInt16LE(input.cameraYaw, 3);
  data.writeUInt8(input.sprint ? 1 : 0, 5);
  data.writeUInt8(input.jump ? 1 : 0, 6);

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: false },
      { pubkey: player, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    programId,
    data,
  });
}

/** Build Attack instruction */
export function buildAttackInstruction(
  world: PublicKey,
  attacker: PublicKey,
  target: PublicKey,
  authority: PublicKey,
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data: [discriminant (1 byte), Option<WeaponStats> as None]
  // Borsh Option::None = single 0 byte
  const data = Buffer.alloc(1 + 1);
  data.writeUInt8(WorldInstructionType.Attack, 0);
  data.writeUInt8(0, 1); // None

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: false },
      { pubkey: attacker, isSigner: false, isWritable: true },
      { pubkey: target, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    programId,
    data,
  });
}

/** Build Heal instruction */
export function buildHealInstruction(
  world: PublicKey,
  player: PublicKey,
  authority: PublicKey,
  amount: number = 0, // 0 = use default
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data: [discriminant (1 byte), amount (2 bytes)]
  const data = Buffer.alloc(1 + 2);
  data.writeUInt8(WorldInstructionType.Heal, 0);
  data.writeUInt16LE(amount, 1);

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: false },
      { pubkey: player, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    programId,
    data,
  });
}

/** Build LeaveWorld instruction */
export function buildLeaveWorldInstruction(
  world: PublicKey,
  player: PublicKey,
  authority: PublicKey,
  rentDestination: PublicKey,
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data: [discriminant (1 byte)] - no payload
  const data = Buffer.alloc(1);
  data.writeUInt8(WorldInstructionType.LeaveWorld, 0);

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: true },
      { pubkey: player, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
      { pubkey: rentDestination, isSigner: false, isWritable: true },
    ],
    programId,
    data,
  });
}

/** Game client for building and sending transactions */
export class GameClient {
  private worldPda: PublicKey;
  private playerPda: PublicKey;
  private keypair: Keypair;
  private programId: PublicKey;

  constructor(
    worldName: string,
    keypair: Keypair,
    programId: PublicKey = WORLD_PROGRAM_ID
  ) {
    this.keypair = keypair;
    this.programId = programId;

    // Derive PDAs
    const [worldPda] = deriveWorldPda(worldName, programId);
    const [playerPda] = deriveWorldPlayerPda(worldPda, keypair.publicKey, programId);

    this.worldPda = worldPda;
    this.playerPda = playerPda;
  }

  get authority(): PublicKey {
    return this.keypair.publicKey;
  }

  get world(): PublicKey {
    return this.worldPda;
  }

  get player(): PublicKey {
    return this.playerPda;
  }

  /** Build join world transaction */
  buildJoinWorld(recentBlockhash: string, playerName: string): Transaction {
    const ix = buildJoinWorldInstruction(
      this.worldPda,
      this.playerPda,
      this.keypair.publicKey,
      this.keypair.publicKey,
      playerName,
      this.programId
    );

    const tx = new Transaction();
    tx.recentBlockhash = recentBlockhash;
    tx.feePayer = this.keypair.publicKey;
    tx.add(ix);
    tx.sign(this.keypair);

    return tx;
  }

  /** Build move player transaction (legacy 2D) */
  buildMove(recentBlockhash: string, direction: Direction, sprint: boolean = false): Transaction {
    const ix = buildMovePlayerInstruction(
      this.worldPda,
      this.playerPda,
      this.keypair.publicKey,
      direction,
      sprint,
      this.programId
    );

    const tx = new Transaction();
    tx.recentBlockhash = recentBlockhash;
    tx.feePayer = this.keypair.publicKey;
    tx.add(ix);
    tx.sign(this.keypair);

    return tx;
  }

  /** Build 3D move player transaction */
  buildMove3D(recentBlockhash: string, input: MovementInput3D): Transaction {
    const ix = buildMovePlayer3DInstruction(
      this.worldPda,
      this.playerPda,
      this.keypair.publicKey,
      input,
      this.programId
    );

    const tx = new Transaction();
    tx.recentBlockhash = recentBlockhash;
    tx.feePayer = this.keypair.publicKey;
    tx.add(ix);
    tx.sign(this.keypair);

    return tx;
  }

  /** Build attack transaction */
  buildAttack(recentBlockhash: string, targetPda: PublicKey): Transaction {
    const ix = buildAttackInstruction(
      this.worldPda,
      this.playerPda,
      targetPda,
      this.keypair.publicKey,
      this.programId
    );

    const tx = new Transaction();
    tx.recentBlockhash = recentBlockhash;
    tx.feePayer = this.keypair.publicKey;
    tx.add(ix);
    tx.sign(this.keypair);

    return tx;
  }

  /** Build heal transaction */
  buildHeal(recentBlockhash: string): Transaction {
    const ix = buildHealInstruction(
      this.worldPda,
      this.playerPda,
      this.keypair.publicKey,
      0,
      this.programId
    );

    const tx = new Transaction();
    tx.recentBlockhash = recentBlockhash;
    tx.feePayer = this.keypair.publicKey;
    tx.add(ix);
    tx.sign(this.keypair);

    return tx;
  }

  /** Build leave world transaction */
  buildLeaveWorld(recentBlockhash: string): Transaction {
    const ix = buildLeaveWorldInstruction(
      this.worldPda,
      this.playerPda,
      this.keypair.publicKey,
      this.keypair.publicKey,
      this.programId
    );

    const tx = new Transaction();
    tx.recentBlockhash = recentBlockhash;
    tx.feePayer = this.keypair.publicKey;
    tx.add(ix);
    tx.sign(this.keypair);

    return tx;
  }

  /** Serialize transaction to base64 */
  static serializeTransaction(tx: Transaction): string {
    return tx.serialize().toString('base64');
  }
}
