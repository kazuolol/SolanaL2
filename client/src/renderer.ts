/**
 * Canvas Renderer - Simple 2D rendering for prototype
 */

import { WorldPlayer, toWorldUnits, Direction, directionToVector } from './state';

export interface RenderConfig {
  canvasWidth: number;
  canvasHeight: number;
  worldWidth: number;
  worldHeight: number;
  playerSize: number;
}

const DEFAULT_CONFIG: RenderConfig = {
  canvasWidth: 800,
  canvasHeight: 600,
  worldWidth: 100,  // World units
  worldHeight: 100,
  playerSize: 20,
};

// Player colors (cycle through for different players)
const PLAYER_COLORS = [
  '#4ade80', // Green (local player)
  '#f87171', // Red
  '#60a5fa', // Blue
  '#fbbf24', // Yellow
  '#a78bfa', // Purple
  '#2dd4bf', // Teal
  '#fb923c', // Orange
  '#f472b6', // Pink
];

export class GameRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private config: RenderConfig;
  private players: Map<string, WorldPlayer> = new Map();
  private localPlayerKey: string | null = null;
  private playerColorMap: Map<string, string> = new Map();
  private nextColorIndex = 1; // 0 is reserved for local player

  constructor(canvas: HTMLCanvasElement, config: Partial<RenderConfig> = {}) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d')!;
    this.config = { ...DEFAULT_CONFIG, ...config };

    // Set canvas size
    this.canvas.width = this.config.canvasWidth;
    this.canvas.height = this.config.canvasHeight;
  }

  /** Set the local player key (for highlighting) */
  setLocalPlayer(playerKey: string): void {
    this.localPlayerKey = playerKey;
    this.playerColorMap.set(playerKey, PLAYER_COLORS[0]);
  }

  /** Update player state */
  updatePlayer(playerKey: string, player: WorldPlayer): void {
    this.players.set(playerKey, player);

    // Assign color if new player
    if (!this.playerColorMap.has(playerKey)) {
      this.playerColorMap.set(playerKey, PLAYER_COLORS[this.nextColorIndex % PLAYER_COLORS.length]);
      this.nextColorIndex++;
    }
  }

  /** Remove player */
  removePlayer(playerKey: string): void {
    this.players.delete(playerKey);
  }

  /** Convert world coordinates to canvas coordinates */
  worldToCanvas(worldX: number, worldY: number): [number, number] {
    const scaleX = this.config.canvasWidth / this.config.worldWidth;
    const scaleY = this.config.canvasHeight / this.config.worldHeight;
    return [worldX * scaleX, worldY * scaleY];
  }

  /** Render frame */
  render(): void {
    const { ctx, config } = this;

    // Clear canvas
    ctx.fillStyle = '#0f0f23';
    ctx.fillRect(0, 0, config.canvasWidth, config.canvasHeight);

    // Draw grid
    this.drawGrid();

    // Draw all players
    for (const [key, player] of this.players) {
      this.drawPlayer(key, player);
    }

    // Draw HUD
    this.drawHUD();
  }

  /** Draw background grid */
  private drawGrid(): void {
    const { ctx, config } = this;
    ctx.strokeStyle = '#1a1a2e';
    ctx.lineWidth = 1;

    const gridSize = 10; // World units per grid cell
    const scaleX = config.canvasWidth / config.worldWidth;
    const scaleY = config.canvasHeight / config.worldHeight;

    // Vertical lines
    for (let x = 0; x <= config.worldWidth; x += gridSize) {
      const canvasX = x * scaleX;
      ctx.beginPath();
      ctx.moveTo(canvasX, 0);
      ctx.lineTo(canvasX, config.canvasHeight);
      ctx.stroke();
    }

    // Horizontal lines
    for (let y = 0; y <= config.worldHeight; y += gridSize) {
      const canvasY = y * scaleY;
      ctx.beginPath();
      ctx.moveTo(0, canvasY);
      ctx.lineTo(config.canvasWidth, canvasY);
      ctx.stroke();
    }
  }

  /** Draw a player */
  private drawPlayer(key: string, player: WorldPlayer): void {
    const { ctx, config } = this;

    // Convert position from fixed-point to world units
    // X/Z is ground plane, Y is vertical (jumping) - for 2D view, use X/Z
    const worldX = toWorldUnits(player.positionX);
    const worldZ = toWorldUnits(player.positionZ);

    // Convert to canvas coordinates (using Z for the vertical canvas axis)
    const [canvasX, canvasY] = this.worldToCanvas(worldX, worldZ);

    const isLocal = key === this.localPlayerKey;
    const color = this.playerColorMap.get(key) || PLAYER_COLORS[0];
    const size = config.playerSize;

    // Draw player body (rectangle)
    ctx.fillStyle = color;
    ctx.fillRect(canvasX - size / 2, canvasY - size / 2, size, size);

    // Draw outline for local player
    if (isLocal) {
      ctx.strokeStyle = '#fff';
      ctx.lineWidth = 2;
      ctx.strokeRect(canvasX - size / 2 - 2, canvasY - size / 2 - 2, size + 4, size + 4);
    }

    // Draw facing direction indicator (from yaw)
    // yaw: 0-65535 maps to 0-360 degrees
    const yawRad = (player.yaw / 65536) * Math.PI * 2;
    const dx = Math.sin(yawRad);
    const dy = -Math.cos(yawRad); // Negative because canvas Y is inverted
    ctx.strokeStyle = color;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(canvasX, canvasY);
    ctx.lineTo(canvasX + dx * size * 0.8, canvasY + dy * size * 0.8);
    ctx.stroke();

    // Draw health bar
    const healthBarWidth = size * 1.5;
    const healthBarHeight = 4;
    const healthPercent = player.health / player.maxHealth;

    // Background
    ctx.fillStyle = '#333';
    ctx.fillRect(
      canvasX - healthBarWidth / 2,
      canvasY - size / 2 - 10,
      healthBarWidth,
      healthBarHeight
    );

    // Health fill
    ctx.fillStyle = healthPercent > 0.5 ? '#4ade80' : healthPercent > 0.25 ? '#fbbf24' : '#f87171';
    ctx.fillRect(
      canvasX - healthBarWidth / 2,
      canvasY - size / 2 - 10,
      healthBarWidth * healthPercent,
      healthBarHeight
    );

    // Draw player name
    ctx.fillStyle = '#fff';
    ctx.font = '12px monospace';
    ctx.textAlign = 'center';
    ctx.fillText(player.name || 'Player', canvasX, canvasY + size / 2 + 14);
  }

  /** Draw HUD */
  private drawHUD(): void {
    const { ctx, config } = this;

    // Draw player count
    ctx.fillStyle = '#888';
    ctx.font = '14px monospace';
    ctx.textAlign = 'left';
    ctx.fillText(`Players: ${this.players.size}`, 10, 20);

    // Draw local player info
    if (this.localPlayerKey) {
      const localPlayer = this.players.get(this.localPlayerKey);
      if (localPlayer) {
        const worldX = toWorldUnits(localPlayer.positionX).toFixed(1);
        const worldZ = toWorldUnits(localPlayer.positionZ).toFixed(1);
        const worldY = toWorldUnits(localPlayer.positionY).toFixed(1);
        ctx.fillText(`Position: (${worldX}, ${worldZ}) Y:${worldY}`, 10, 40);
        ctx.fillText(`Health: ${localPlayer.health}/${localPlayer.maxHealth}`, 10, 60);
      }
    }
  }

  /** Start render loop */
  startRenderLoop(): void {
    const loop = () => {
      this.render();
      requestAnimationFrame(loop);
    };
    loop();
  }
}
