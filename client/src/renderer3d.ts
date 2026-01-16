/**
 * Three.js 3D Renderer
 *
 * Third-person camera with mouse look, ground plane, player capsules.
 */

import * as THREE from 'three';
import { FIXED_POINT_SCALE } from './state';

export interface Renderer3DConfig {
  worldWidth: number;
  worldDepth: number;
  cameraDistance: number;
  cameraHeight: number;
}

const DEFAULT_CONFIG: Renderer3DConfig = {
  worldWidth: 100,
  worldDepth: 100,
  cameraDistance: 8,
  cameraHeight: 4,
};

// Player colors
const PLAYER_COLORS = [
  0x4ade80, // Green (local player)
  0xf87171, // Red
  0x60a5fa, // Blue
  0xfbbf24, // Yellow
  0xa78bfa, // Purple
  0x2dd4bf, // Teal
  0xfb923c, // Orange
  0xf472b6, // Pink
];

interface PlayerMesh {
  group: THREE.Group;
  body: THREE.Mesh;
  nameSprite: THREE.Sprite;
}

export class Renderer3D {
  private container: HTMLElement;
  private scene: THREE.Scene;
  private camera: THREE.PerspectiveCamera;
  private renderer: THREE.WebGLRenderer;
  private config: Renderer3DConfig;

  // Camera control
  private cameraYaw = 0; // Horizontal rotation (0-2PI)
  private cameraPitch = 0.3; // Vertical rotation (-PI/2 to PI/2)
  private isPointerLocked = false;

  // Players
  private players: Map<string, PlayerMesh> = new Map();
  private localPlayerKey: string | null = null;
  private playerColorMap: Map<string, number> = new Map();
  private nextColorIndex = 1;

  // Local player position for camera following
  private localPlayerPosition = new THREE.Vector3(50, 0, 50);

  constructor(container: HTMLElement, config: Partial<Renderer3DConfig> = {}) {
    this.container = container;
    this.config = { ...DEFAULT_CONFIG, ...config };

    // Create scene
    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color(0x1a1a2e);
    this.scene.fog = new THREE.Fog(0x1a1a2e, 50, 150);

    // Create camera
    this.camera = new THREE.PerspectiveCamera(
      60,
      container.clientWidth / container.clientHeight,
      0.1,
      1000
    );

    // Create renderer
    this.renderer = new THREE.WebGLRenderer({ antialias: true });
    this.renderer.setSize(container.clientWidth, container.clientHeight);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    container.appendChild(this.renderer.domElement);

    // Setup scene
    this.setupLighting();
    this.setupGround();

    // Setup controls
    this.setupPointerLock();
    this.setupResize();
  }

  private setupLighting(): void {
    // Ambient light
    const ambient = new THREE.AmbientLight(0x404060, 0.5);
    this.scene.add(ambient);

    // Directional light (sun)
    const sun = new THREE.DirectionalLight(0xffffff, 1);
    sun.position.set(50, 100, 50);
    sun.castShadow = true;
    sun.shadow.mapSize.width = 2048;
    sun.shadow.mapSize.height = 2048;
    sun.shadow.camera.near = 10;
    sun.shadow.camera.far = 200;
    sun.shadow.camera.left = -50;
    sun.shadow.camera.right = 50;
    sun.shadow.camera.top = 50;
    sun.shadow.camera.bottom = -50;
    this.scene.add(sun);

    // Hemisphere light for softer shadows
    const hemi = new THREE.HemisphereLight(0x87ceeb, 0x2d4a2d, 0.3);
    this.scene.add(hemi);
  }

  private setupGround(): void {
    const { worldWidth, worldDepth } = this.config;

    // Ground plane
    const groundGeom = new THREE.PlaneGeometry(worldWidth, worldDepth);
    const groundMat = new THREE.MeshStandardMaterial({
      color: 0x2d4a2d,
      roughness: 0.9,
    });
    const ground = new THREE.Mesh(groundGeom, groundMat);
    ground.rotation.x = -Math.PI / 2;
    ground.position.set(worldWidth / 2, 0, worldDepth / 2);
    ground.receiveShadow = true;
    this.scene.add(ground);

    // Grid overlay
    const gridHelper = new THREE.GridHelper(
      Math.max(worldWidth, worldDepth),
      Math.max(worldWidth, worldDepth) / 10,
      0x4a4a6a,
      0x3a3a4a
    );
    gridHelper.position.set(worldWidth / 2, 0.01, worldDepth / 2);
    this.scene.add(gridHelper);

    // World boundary markers
    const boundaryMat = new THREE.MeshBasicMaterial({ color: 0x9d4edd });
    const markerGeom = new THREE.BoxGeometry(0.5, 2, 0.5);
    const corners = [
      [0, 0], [worldWidth, 0],
      [0, worldDepth], [worldWidth, worldDepth]
    ];
    for (const [x, z] of corners) {
      const marker = new THREE.Mesh(markerGeom, boundaryMat);
      marker.position.set(x, 1, z);
      this.scene.add(marker);
    }
  }

  private setupPointerLock(): void {
    const canvas = this.renderer.domElement;

    canvas.addEventListener('click', () => {
      canvas.requestPointerLock();
    });

    document.addEventListener('pointerlockchange', () => {
      this.isPointerLocked = document.pointerLockElement === canvas;
    });

    document.addEventListener('mousemove', (e) => {
      if (!this.isPointerLocked) return;

      // Horizontal mouse movement -> yaw (negated for natural camera control)
      this.cameraYaw -= e.movementX * 0.002;
      // Keep yaw in 0-2PI range
      while (this.cameraYaw < 0) this.cameraYaw += Math.PI * 2;
      while (this.cameraYaw >= Math.PI * 2) this.cameraYaw -= Math.PI * 2;

      // Vertical mouse movement -> pitch (limited)
      this.cameraPitch += e.movementY * 0.002;
      this.cameraPitch = Math.max(-Math.PI / 3, Math.min(Math.PI / 3, this.cameraPitch));
    });
  }

  private setupResize(): void {
    window.addEventListener('resize', () => {
      const width = this.container.clientWidth;
      const height = this.container.clientHeight;
      this.camera.aspect = width / height;
      this.camera.updateProjectionMatrix();
      this.renderer.setSize(width, height);
    });
  }

  /** Set the local player key */
  setLocalPlayer(playerKey: string): void {
    this.localPlayerKey = playerKey;
    this.playerColorMap.set(playerKey, PLAYER_COLORS[0]);
  }

  /** Update player state */
  updatePlayer(playerKey: string, data: {
    positionX: number;
    positionZ: number;
    positionY: number;
    yaw: number;
    health: number;
    maxHealth: number;
    name: string;
  }): void {
    // Convert from fixed-point to world units
    const worldX = data.positionX / FIXED_POINT_SCALE;
    const worldZ = data.positionZ / FIXED_POINT_SCALE;
    const worldY = data.positionY / FIXED_POINT_SCALE;

    console.log(`[Renderer] updatePlayer: key=${playerKey.slice(0, 8)}..., pos=(${worldX.toFixed(2)}, ${worldY.toFixed(2)}, ${worldZ.toFixed(2)})`);

    // Get or create player mesh
    let playerMesh = this.players.get(playerKey);
    if (!playerMesh) {
      console.log(`[Renderer] Creating new player mesh for ${playerKey.slice(0, 8)}...`);
      playerMesh = this.createPlayerMesh(playerKey);
      this.players.set(playerKey, playerMesh);
      console.log(`[Renderer] Player mesh created and added to scene`);
    }

    // Update position
    playerMesh.group.position.set(worldX, worldY, worldZ);
    console.log(`[Renderer] Mesh position set to: (${playerMesh.group.position.x.toFixed(2)}, ${playerMesh.group.position.y.toFixed(2)}, ${playerMesh.group.position.z.toFixed(2)})`);

    // Update rotation (yaw: 0-65535 -> 0-2PI)
    const yawRad = (data.yaw / 65536) * Math.PI * 2;
    playerMesh.body.rotation.y = -yawRad;

    // Update color based on health
    const bodyMat = playerMesh.body.material as THREE.MeshStandardMaterial;
    const healthPercent = data.health / data.maxHealth;
    if (healthPercent < 0.25) {
      bodyMat.emissive.setHex(0x550000);
    } else if (healthPercent < 0.5) {
      bodyMat.emissive.setHex(0x553300);
    } else {
      bodyMat.emissive.setHex(0x000000);
    }

    // Update local player position for camera
    if (playerKey === this.localPlayerKey) {
      this.localPlayerPosition.set(worldX, worldY, worldZ);
    }
  }

  private createPlayerMesh(playerKey: string): PlayerMesh {
    const isLocal = playerKey === this.localPlayerKey;
    console.log(`[Renderer] createPlayerMesh: key=${playerKey.slice(0, 8)}..., isLocal=${isLocal}, localPlayerKey=${this.localPlayerKey?.slice(0, 8) ?? 'null'}`);

    // Assign color
    if (!this.playerColorMap.has(playerKey)) {
      this.playerColorMap.set(playerKey, PLAYER_COLORS[this.nextColorIndex % PLAYER_COLORS.length]);
      this.nextColorIndex++;
    }
    const color = this.playerColorMap.get(playerKey)!;
    console.log(`[Renderer] Player color: 0x${color.toString(16)} (${isLocal ? 'green/local' : 'other'})`);

    // Create group
    const group = new THREE.Group();

    // Create body (capsule approximation with cylinder + spheres)
    const bodyGeom = new THREE.CapsuleGeometry(0.3, 1.2, 4, 8);
    const bodyMat = new THREE.MeshStandardMaterial({
      color: color,
      roughness: 0.5,
      metalness: 0.2,
    });
    const body = new THREE.Mesh(bodyGeom, bodyMat);
    body.position.y = 0.9; // Center of capsule
    body.castShadow = true;
    group.add(body);

    // Direction indicator (small cone pointing forward)
    const dirGeom = new THREE.ConeGeometry(0.15, 0.4, 8);
    const dirMat = new THREE.MeshStandardMaterial({ color: 0xffffff });
    const direction = new THREE.Mesh(dirGeom, dirMat);
    direction.rotation.x = Math.PI / 2;
    direction.position.set(0, 1.2, -0.5);
    body.add(direction);

    // Outline ring for local player
    if (isLocal) {
      const ringGeom = new THREE.RingGeometry(0.5, 0.6, 32);
      const ringMat = new THREE.MeshBasicMaterial({
        color: 0xffffff,
        side: THREE.DoubleSide,
      });
      const ring = new THREE.Mesh(ringGeom, ringMat);
      ring.rotation.x = -Math.PI / 2;
      ring.position.y = 0.02;
      group.add(ring);
    }

    // Name sprite (placeholder - just a colored sphere above head)
    const nameMat = new THREE.SpriteMaterial({ color: 0xffffff });
    const nameSprite = new THREE.Sprite(nameMat);
    nameSprite.position.y = 2.2;
    nameSprite.scale.set(0.5, 0.25, 1);
    group.add(nameSprite);

    this.scene.add(group);

    return { group, body, nameSprite };
  }

  /** Remove player */
  removePlayer(playerKey: string): void {
    const mesh = this.players.get(playerKey);
    if (mesh) {
      this.scene.remove(mesh.group);
      this.players.delete(playerKey);
    }
  }

  /** Get camera yaw as i16 (0-65535 maps to 0-360 degrees) */
  getCameraYawInt16(): number {
    // Convert 0-2PI to 0-65535
    return Math.floor((this.cameraYaw / (Math.PI * 2)) * 65536) & 0xFFFF;
  }

  /** Check if pointer is locked (for UI hints) */
  isLocked(): boolean {
    return this.isPointerLocked;
  }

  /** Render frame */
  render(): void {
    // Update camera position (third-person follow)
    const { cameraDistance, cameraHeight } = this.config;

    // Camera offset based on yaw and pitch
    const offsetX = Math.sin(this.cameraYaw) * Math.cos(this.cameraPitch) * cameraDistance;
    const offsetZ = Math.cos(this.cameraYaw) * Math.cos(this.cameraPitch) * cameraDistance;
    const offsetY = Math.sin(this.cameraPitch) * cameraDistance + cameraHeight;

    this.camera.position.set(
      this.localPlayerPosition.x + offsetX,
      this.localPlayerPosition.y + offsetY,
      this.localPlayerPosition.z + offsetZ
    );

    // Look at player
    this.camera.lookAt(
      this.localPlayerPosition.x,
      this.localPlayerPosition.y + 1, // Look at chest height
      this.localPlayerPosition.z
    );

    this.renderer.render(this.scene, this.camera);
  }

  /** Start render loop */
  startRenderLoop(): void {
    const loop = () => {
      this.render();
      requestAnimationFrame(loop);
    };
    loop();
  }

  /** Dispose of resources */
  dispose(): void {
    this.renderer.dispose();
    this.container.removeChild(this.renderer.domElement);
  }
}
