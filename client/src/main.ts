/**
 * L2 Game Client - Main Entry Point (3D Version)
 *
 * Uses REAL Solana transactions routed through SVM.
 * Third-person 3D movement with mouse look and WASD controls.
 */

import { Keypair, PublicKey } from '@solana/web3.js';
import { L2Connection } from './connection';
import { Renderer3D } from './renderer3d';
import { GameClient, MovementInput3D } from './transaction';
import {
  FIXED_POINT_SCALE,
  WORLD_PROGRAM_ID,
  deriveWorldPda,
  deriveWorldPlayerPda,
  decodeWorldPlayer,
  WorldPlayer,
} from './state';

// Configuration
const RPC_URL = 'http://127.0.0.1:8899';
const WS_URL = 'ws://127.0.0.1:8900';
const DEFAULT_WORLD_NAME = 'default';

// UI Elements
const statusEl = document.getElementById('status')!;
const debugEl = document.getElementById('debug')!;
const gameContainer = document.getElementById('game-container')!;
const controlsHint = document.getElementById('controls-hint')!;

// Game state
let connection: L2Connection;
let renderer: Renderer3D;
let keypair: Keypair;
let gameClient: GameClient;
let isJoined = false;
let currentBlockhash: string = '';

// Key state tracking
const keysPressed = new Set<string>();

// Movement input loop
let inputLoopId: number | null = null;

// Debug logging
function log(msg: string): void {
  const time = new Date().toLocaleTimeString();
  debugEl.innerHTML = `[${time}] ${msg}\n` + debugEl.innerHTML;
  if (debugEl.innerHTML.length > 5000) {
    debugEl.innerHTML = debugEl.innerHTML.slice(0, 5000);
  }
}

// Transaction log element
const txLogEl = document.getElementById('tx-log')!;
let txCount = 0;

// Log a transaction
function logTx(action: string, slot: number, signature: string, account?: string): void {
  txCount++;
  const time = new Date().toLocaleTimeString();
  const shortSig = signature.slice(0, 12);
  const shortAccount = account ? account.slice(0, 8) + '...' : '';

  const entry = document.createElement('div');
  entry.className = 'tx-entry';
  entry.innerHTML = `<span class="tx-slot">slot ${slot}</span> <span class="tx-action">${action}</span> <span class="tx-sig">${shortSig}</span> ${shortAccount}`;

  txLogEl.insertBefore(entry, txLogEl.firstChild);

  // Keep only last 50 entries
  while (txLogEl.children.length > 50) {
    txLogEl.removeChild(txLogEl.lastChild!);
  }
}

// Update status indicator
function setStatus(status: 'connected' | 'disconnected' | 'connecting'): void {
  statusEl.className = status;
  statusEl.textContent = status.charAt(0).toUpperCase() + status.slice(1);
}

// Fetch latest blockhash
async function refreshBlockhash(): Promise<void> {
  try {
    const result = await connection.rpc<{ value: { blockhash: string } }>('getLatestBlockhash', []);
    if (result?.value?.blockhash) {
      currentBlockhash = result.value.blockhash;
    }
  } catch (e) {
    log(`Failed to get blockhash: ${e}`);
  }
}

// Get player state from account data
async function getPlayerState(): Promise<WorldPlayer | null> {
  try {
    const playerPda = gameClient.player;
    const result = await connection.rpc<{ value: { data: [string, string] } | null }>('getAccountInfo', [
      playerPda.toBase58(),
      { encoding: 'base64' }
    ]);

    if (!result?.value?.data) {
      return null;
    }

    const data = Buffer.from(result.value.data[0], 'base64');
    return decodeWorldPlayer(data);
  } catch (e) {
    return null;
  }
}

// Get all players by scanning program accounts
async function getAllPlayers(): Promise<{ pda: string; player: WorldPlayer }[]> {
  // For now, just get our own player since getProgramAccounts isn't implemented
  // In a full implementation, we'd use getProgramAccounts
  const player = await getPlayerState();
  if (player) {
    return [{ pda: gameClient.player.toBase58(), player }];
  }
  return [];
}

// Update renderer from player state
function updatePlayerFromState(player: WorldPlayer): void {
  if (!player) return;

  renderer.updatePlayer(keypair.publicKey.toBase58(), {
    positionX: player.positionX,
    positionZ: player.positionZ,
    positionY: player.positionY,
    yaw: player.yaw,
    health: player.health,
    maxHealth: player.maxHealth,
    name: player.name,
  });
}

// Initialize
async function init(): Promise<void> {
  log('Initializing 3D game client (REAL TRANSACTIONS)...');

  // Generate or load keypair
  const storedKey = localStorage.getItem('l2_keypair');
  if (storedKey) {
    keypair = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(storedKey)));
    log(`Loaded keypair: ${keypair.publicKey.toBase58().slice(0, 8)}...`);
  } else {
    keypair = Keypair.generate();
    localStorage.setItem('l2_keypair', JSON.stringify(Array.from(keypair.secretKey)));
    log(`Generated new keypair: ${keypair.publicKey.toBase58().slice(0, 8)}...`);
  }

  // Create game client for building transactions
  gameClient = new GameClient(DEFAULT_WORLD_NAME, keypair, WORLD_PROGRAM_ID);
  log(`World PDA: ${gameClient.world.toBase58().slice(0, 8)}...`);
  log(`Player PDA: ${gameClient.player.toBase58().slice(0, 8)}...`);

  // Create connection
  connection = new L2Connection(RPC_URL, WS_URL, setStatus);

  // Create 3D renderer
  renderer = new Renderer3D(gameContainer, {
    worldWidth: 100,
    worldDepth: 100,
  });

  // Connect WebSocket
  try {
    await connection.connect();
    log('WebSocket connected');
  } catch (e) {
    log(`WebSocket error: ${e}`);
  }

  // Get initial blockhash
  await refreshBlockhash();
  log(`Initial blockhash: ${currentBlockhash.slice(0, 8)}...`);

  // Refresh blockhash periodically
  setInterval(refreshBlockhash, 2000);

  // Check if already joined by querying account
  try {
    const player = await getPlayerState();
    if (player) {
      isJoined = true;
      renderer.setLocalPlayer(keypair.publicKey.toBase58());
      updatePlayerFromState(player);
      log('Found existing player - already joined!');
      startInputLoop();
    }
  } catch (e) {
    log(`Not yet joined`);
  }

  // Start render loop
  renderer.startRenderLoop();

  // Start game loop (poll for state updates)
  startGameLoop();

  // Set up input handling
  setupInput();

  updateControlsHint();
  log('Ready! Press J to join the world, click to enable mouse look.');
}

// Update controls hint based on state
function updateControlsHint(): void {
  if (!isJoined) {
    controlsHint.textContent = 'Press J to join the world';
  } else if (!renderer.isLocked()) {
    controlsHint.textContent = 'Click to enable mouse look | WASD to move | Space to jump | Shift to sprint';
  } else {
    controlsHint.textContent = 'WASD to move | Space to jump | Shift to sprint | ESC to release mouse';
  }
}

// Game loop - poll for state updates
function startGameLoop(): void {
  setInterval(async () => {
    if (!isJoined) return;

    try {
      const player = await getPlayerState();
      if (player) {
        updatePlayerFromState(player);
      }
    } catch (e) {
      // Ignore polling errors
    }

    updateControlsHint();
  }, 100); // Poll every 100ms
}

// Set up keyboard input
function setupInput(): void {
  document.addEventListener('keydown', (e) => {
    const key = e.key.toLowerCase();
    keysPressed.add(key);

    // J to join (instead of Space, so Space can be jump)
    if (key === 'j' && !isJoined) {
      joinWorld();
      e.preventDefault();
    }
  });

  document.addEventListener('keyup', (e) => {
    keysPressed.delete(e.key.toLowerCase());
  });
}

// Get 3D movement input from keys
function getMovementInput(): MovementInput3D {
  const forward = keysPressed.has('w') || keysPressed.has('arrowup');
  const backward = keysPressed.has('s') || keysPressed.has('arrowdown');
  const left = keysPressed.has('a') || keysPressed.has('arrowleft');
  const right = keysPressed.has('d') || keysPressed.has('arrowright');
  const sprint = keysPressed.has('shift');
  const jump = keysPressed.has(' ');

  // Calculate camera-relative movement (-127 to 127)
  let moveX = 0;
  let moveZ = 0;

  if (forward) moveZ -= 127;   // Forward = -Z
  if (backward) moveZ += 127;  // Backward = +Z
  if (right) moveX += 127;
  if (left) moveX -= 127;

  // Normalize diagonal movement
  if (moveX !== 0 && moveZ !== 0) {
    const scale = 127 / Math.sqrt(moveX * moveX + moveZ * moveZ);
    moveX = Math.round(moveX * scale);
    moveZ = Math.round(moveZ * scale);
  }

  return {
    moveX,
    moveZ,
    cameraYaw: renderer.getCameraYawInt16(),
    sprint,
    jump,
  };
}

// Input loop - continuously send movement transactions
function startInputLoop(): void {
  if (inputLoopId !== null) return;

  const TICK_RATE = 30; // 30 Hz to match server
  const TICK_MS = 1000 / TICK_RATE;

  inputLoopId = window.setInterval(async () => {
    if (!isJoined || !currentBlockhash) return;

    const input = getMovementInput();

    // Only send if there's movement or jump
    if (input.moveX !== 0 || input.moveZ !== 0 || input.jump) {
      await sendMove3D(input);
    }
  }, TICK_MS);
}

// Join world using REAL transaction
async function joinWorld(): Promise<void> {
  if (isJoined) return;

  log('Joining world with REAL transaction...');

  try {
    // Refresh blockhash first
    await refreshBlockhash();
    if (!currentBlockhash) {
      log('No blockhash available');
      return;
    }

    const playerName = `Player${Math.floor(Math.random() * 1000)}`;

    // Build and sign the transaction
    const tx = gameClient.buildJoinWorld(currentBlockhash, playerName);
    const txBase64 = GameClient.serializeTransaction(tx);

    log(`Sending JoinWorld tx: ${tx.signature?.toString().slice(0, 12)}...`);

    // Send via RPC
    const signature = await connection.rpc<string>('sendTransaction', [txBase64]);

    if (signature) {
      const slot = await connection.rpc<number>('getSlot', []) || 0;
      logTx('JoinWorld', slot, signature, gameClient.player.toBase58());
      log(`Joined! Signature: ${signature.slice(0, 12)}...`);

      isJoined = true;
      renderer.setLocalPlayer(keypair.publicKey.toBase58());

      // Wait a moment for transaction to process, then get state
      setTimeout(async () => {
        const player = await getPlayerState();
        if (player) {
          updatePlayerFromState(player);
          const x = (player.positionX / FIXED_POINT_SCALE).toFixed(1);
          const z = (player.positionZ / FIXED_POINT_SCALE).toFixed(1);
          log(`Spawned at (${x}, ${z})`);
        }
      }, 200);

      // Start input loop
      startInputLoop();
    }
  } catch (e) {
    log(`Join error: ${e}`);
  }
}

// Send 3D move using REAL transaction
async function sendMove3D(input: MovementInput3D): Promise<void> {
  try {
    if (!currentBlockhash) return;

    // Build and sign the transaction
    const tx = gameClient.buildMove3D(currentBlockhash, input);
    const txBase64 = GameClient.serializeTransaction(tx);

    // Send via RPC (fire and forget for movement)
    const signature = await connection.rpc<string>('sendTransaction', [txBase64]);

    // Log the transaction (only occasionally to avoid spam)
    if (signature && txCount % 10 === 0) {
      const slot = await connection.rpc<number>('getSlot', []) || 0;
      logTx('Move3D', slot, signature, gameClient.player.toBase58());
    }
    txCount++;
  } catch (e) {
    // Silently ignore move errors to avoid log spam
  }
}

// Start the app
init().catch((e) => {
  log(`Init error: ${e}`);
  console.error(e);
});
