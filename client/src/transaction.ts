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

/** Instruction discriminants (matches WorldInstruction enum) */
enum WorldInstructionType {
  InitializeWorld = 0,
  JoinWorld = 1,
  MovePlayer = 2,
  Attack = 3,
  Heal = 4,
  LeaveWorld = 5,
  UpdateWorld = 6,
  SetPvpZone = 7,
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
  // Instruction data: [type (1 byte), name (16 bytes)]
  const data = Buffer.alloc(1 + 16);
  data.writeUInt8(WorldInstructionType.JoinWorld, 0);

  const nameBuffer = Buffer.alloc(16);
  nameBuffer.write(playerName.slice(0, 16));
  nameBuffer.copy(data, 1);

  return new TransactionInstruction({
    keys: [
      { pubkey: world, isSigner: false, isWritable: false },
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
  // Instruction data: [type (1 byte), direction (1 byte), sprint (1 byte)]
  const data = Buffer.alloc(3);
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

/** Build Attack instruction */
export function buildAttackInstruction(
  world: PublicKey,
  attacker: PublicKey,
  target: PublicKey,
  authority: PublicKey,
  programId: PublicKey = WORLD_PROGRAM_ID
): TransactionInstruction {
  // Instruction data: [type (1 byte), Option<WeaponStats> as None]
  // None = 0 byte
  const data = Buffer.alloc(2);
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
  // Instruction data: [type (1 byte), amount (2 bytes)]
  const data = Buffer.alloc(3);
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

  /** Build move player transaction */
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
