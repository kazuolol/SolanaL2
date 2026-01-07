/**
 * L2 Game Client - Main Entry Point (3D Version)
 *
 * Third-person 3D movement with mouse look and WASD controls.
 * Uses game_move3d RPC for camera-relative movement.
 */

import { Keypair } from '@solana/web3.js';
import { L2Connection } from './connection';
import { Renderer3D } from './renderer3d';
import { FIXED_POINT_SCALE, MovementInput3D } from './state';

// Configuration
const RPC_URL = 'http://127.0.0.1:8899';
const WS_URL = 'ws://127.0.0.1:8900';

// UI Elements
const statusEl = document.getElementById('status')!;
const debugEl = document.getElementById('debug')!;
const gameContainer = document.getElementById('game-container')!;
const controlsHint = document.getElementById('controls-hint')!;

// Game state
let connection: L2Connection;
let renderer: Renderer3D;
let keypair: Keypair;
let playerPda: string | null = null;
let isJoined = false;

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

// Log a transaction/write to the chain
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

// Initialize
async function init(): Promise<void> {
  log('Initializing 3D game client...');

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

  // Check if already joined by querying game state
  try {
    const player = await getPlayerState();
    if (player) {
      isJoined = true;
      playerPda = 'derived';
      renderer.setLocalPlayer(keypair.publicKey.toBase58());
      updatePlayerFromState(player);
      log('Found existing player - already joined!');
      startInputLoop();
    }
  } catch (e) {
    log(`Not yet joined: ${e}`);
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

// Get player state from server
async function getPlayerState(): Promise<any | null> {
  const result = await connection.rpc<any>('game_getPlayer', [
    keypair.publicKey.toBase58()
  ]);
  return result;
}

// Get all players from server
async function getAllPlayers(): Promise<any[]> {
  const result = await connection.rpc<any[]>('game_getAllPlayers', []);
  return result || [];
}

// Update renderer from player state
function updatePlayerFromState(player: any): void {
  if (!player) return;

  renderer.updatePlayer(keypair.publicKey.toBase58(), {
    positionX: player.positionX,
    positionZ: player.positionZ,
    positionY: player.positionY,
    yaw: player.yaw,
    health: player.health,
    maxHealth: player.maxHealth,
    name: player.name || 'Player',
  });
}

// Update all players in renderer
function updateAllPlayers(players: any[]): void {
  const localKey = keypair.publicKey.toBase58();

  for (const player of players) {
    // Use authority as the player key for identification
    const playerKey = player.authority;

    // Set local player if this is us
    if (playerKey === localKey && !renderer.isLocked()) {
      renderer.setLocalPlayer(localKey);
    }

    renderer.updatePlayer(playerKey, {
      positionX: player.positionX,
      positionZ: player.positionZ,
      positionY: player.positionY,
      yaw: player.yaw,
      health: player.health,
      maxHealth: player.maxHealth,
      name: player.name || 'Player',
    });
  }
}

// Game loop - poll for state updates
function startGameLoop(): void {
  setInterval(async () => {
    if (!isJoined) return;

    try {
      // Fetch all players to see other players
      const players = await getAllPlayers();
      if (players.length > 0) {
        updateAllPlayers(players);
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
  // In Three.js, -Z is forward (into screen), +Z is backward
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

// Input loop - continuously send movement
function startInputLoop(): void {
  if (inputLoopId !== null) return;

  const TICK_RATE = 30; // 30 Hz to match server
  const TICK_MS = 1000 / TICK_RATE;

  inputLoopId = window.setInterval(async () => {
    if (!isJoined) return;

    const input = getMovementInput();

    // Only send if there's movement or jump
    if (input.moveX !== 0 || input.moveZ !== 0 || input.jump) {
      await sendMove3D(input);
    }
  }, TICK_MS);
}

// Join world using simplified RPC
async function joinWorld(): Promise<void> {
  if (isJoined) return;

  log('Joining world...');

  try {
    const playerName = `Player${Math.floor(Math.random() * 1000)}`;
    const result = await connection.rpc<any>('game_joinWorld', [
      keypair.publicKey.toBase58(),
      playerName
    ]);

    if (result && result.playerPda) {
      playerPda = result.playerPda;
      isJoined = true;
      renderer.setLocalPlayer(keypair.publicKey.toBase58());
      log(`Joined! Player PDA: ${result.playerPda.slice(0, 8)}...`);

      // Log the join transaction
      if (result.signature) {
        logTx(result.action, result.slot, result.signature, result.playerPda);
      }

      // Get initial state
      const player = await getPlayerState();
      if (player) {
        updatePlayerFromState(player);
        const x = (player.positionX / FIXED_POINT_SCALE).toFixed(1);
        const z = (player.positionZ / FIXED_POINT_SCALE).toFixed(1);
        log(`Spawned at (${x}, ${z})`);
      }

      // Start input loop
      startInputLoop();
    }
  } catch (e) {
    log(`Join error: ${e}`);
  }
}

// Send 3D move using game_move3d RPC
async function sendMove3D(input: MovementInput3D): Promise<void> {
  try {
    const result = await connection.rpc<any>('game_move3d', [
      keypair.publicKey.toBase58(),
      input.moveX,
      input.moveZ,
      input.cameraYaw,
      input.sprint,
      input.jump,
    ]);

    // Log the transaction
    if (result && result.signature) {
      logTx(result.action, result.slot, result.signature, result.account);
    }
  } catch (e) {
    // Silently ignore move errors to avoid log spam
  }
}

// Start the app
init().catch((e) => {
  log(`Init error: ${e}`);
  console.error(e);
});
