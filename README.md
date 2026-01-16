# SolanaL2 - Real-Time Gaming Chain

A high-performance Layer 2 gaming chain built on the Solana Virtual Machine (SVM). Runs a 30Hz game loop with gasless transactions and fully on-chain game state.

## What is this?

This is a **real Solana Virtual Machine** running locally as a game server. Every player movement, jump, and action is a signed transaction processed by the same SVM that powers Solana mainnet - just optimized for real-time gaming:

- **30Hz tick rate** (33ms blocks) - matches typical game servers
- **Gasless transactions** - no fees for gameplay
- **On-chain state** - player positions stored as Solana accounts
- **Native builtin** - game logic runs as native Rust, not BPF bytecode

## Quick Start

```bash
# Terminal 1: Start the L2 server
cargo run --release --bin solana-l2

# Terminal 2: Start the client
cd client
npm install
npm run dev
```

Open **http://localhost:3000** in your browser. Press **J** to join the world, then use **WASD** to move and **Space** to jump.

## Architecture

```
┌─────────────────┐                    ┌──────────────────┐
│  Browser Client │ ── HTTP POST ────▶ │   RPC Server     │
│  (Three.js 3D)  │     :8899          │   (Axum)         │
│                 │ ◀── WebSocket ──── │   :8900          │
└─────────────────┘                    └────────┬─────────┘
                                                │
                                                ▼
┌───────────────────────────────────────────────────────────┐
│                    Block Producer (30Hz)                   │
│  Drains transaction queue every 33ms, max 64 txs/block    │
└────────────────────────────┬──────────────────────────────┘
                             │
                             ▼
┌───────────────────────────────────────────────────────────┐
│                      Solana SVM                            │
│  TransactionBatchProcessor from solana-svm crate          │
│  - Loads accounts via L2AccountLoader callback            │
│  - Executes world_program builtin                         │
│  - Returns modified accounts                              │
└────────────────────────────┬──────────────────────────────┘
                             │
                             ▼
┌───────────────────────────────────────────────────────────┐
│                     Account Store                          │
│  In-memory: DashMap    │    On-disk: Sled (every 300 slots)│
└───────────────────────────────────────────────────────────┘
```

## Project Structure

```
SolanaL2/
├── crates/
│   ├── validator/           # Main binary entry point
│   │   └── src/main.rs      # CLI args, server orchestration
│   │
│   ├── l2-runtime/          # Core SVM execution engine
│   │   └── src/
│   │       ├── processor.rs      # Wraps TransactionBatchProcessor
│   │       ├── callback.rs       # L2AccountLoader (account creation)
│   │       ├── block_producer.rs # 30Hz game loop
│   │       ├── account_store.rs  # DashMap storage
│   │       ├── persistence.rs    # Sled disk persistence
│   │       └── tests/            # Integration tests
│   │
│   ├── world-program/       # Game logic (native builtin)
│   │   └── src/
│   │       ├── builtin.rs        # Instruction processor
│   │       ├── instruction.rs    # JoinWorld, Movement, etc.
│   │       ├── state.rs          # WorldConfig, WorldPlayer
│   │       └── constants.rs      # Seeds, physics values
│   │
│   ├── rpc-server/          # JSON-RPC API
│   │   └── src/
│   │       ├── http_server.rs    # POST /rpc on :8899
│   │       ├── ws_server.rs      # WebSocket on :8900
│   │       └── methods.rs        # getAccountInfo, sendTransaction
│   │
│   └── l2-consensus/        # Leader/validator networking
│       └── src/
│           ├── leader.rs         # Broadcasts state updates
│           └── broadcast.rs      # UDP multicast
│
├── client/                  # Browser game client
│   └── src/
│       ├── main.ts              # Entry point, game loop
│       ├── game.ts              # GameClient, transaction building
│       ├── connection.ts        # L2Connection (RPC + WebSocket)
│       ├── renderer3d.ts        # Three.js rendering
│       └── state.ts             # WorldPlayer deserialization
│
├── CLAUDE.md                # AI coding assistant instructions
└── README.md                # This file
```

## Key Files

| File | Purpose |
|------|---------|
| `crates/l2-runtime/src/callback.rs` | Creates accounts on-the-fly. PDAs get `world_program` as owner. |
| `crates/l2-runtime/src/processor.rs` | Wraps Solana's `TransactionBatchProcessor` for execution. |
| `crates/world-program/src/builtin.rs` | Game logic: join world, movement, physics. Native Rust, not BPF. |
| `crates/world-program/src/state.rs` | `WorldConfig` (118 bytes) and `WorldPlayer` (123 bytes) structs. |
| `client/src/game.ts` | Builds and signs transactions, derives PDAs. |

## How It Works

### 1. Player Joins
```
Client: Creates JoinWorld transaction with player PDA
Server: L2AccountLoader auto-creates PDA owned by world_program
SVM:    Executes builtin, writes WorldPlayer struct to PDA
Client: Polls getAccountInfo until player account exists
```

### 2. Player Moves
```
Client: Sends PlayerMovement transaction (30Hz)
Server: Queues transaction, processes in next block
SVM:    Builtin updates position/velocity in WorldPlayer
Client: Reads updated position, renders new location
```

### 3. Account Storage
```
- Missing wallets: Created with system_program owner, 0 bytes
- Missing PDAs: Created with world_program owner, 123 bytes
- This allows the world program to write to PDAs without CPI
```

## Network Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 8899 | HTTP | JSON-RPC API (sendTransaction, getAccountInfo) |
| 8900 | WebSocket | Account subscriptions (future) |
| 9000 | UDP | Leader → Validator state broadcast |
| 3000 | HTTP | Vite dev server (client) |

## Running Tests

```bash
# All l2-runtime tests (16 tests)
cargo test -p l2-runtime

# Just the JoinWorld integration tests (8 tests)
cargo test -p l2-runtime join_world
```

## Configuration

The server uses these defaults (no config file needed):

| Setting | Value | Description |
|---------|-------|-------------|
| Block time | 33ms | 30Hz tick rate |
| Max txs/block | 64 | Throughput limit |
| Save interval | 300 slots | ~10 seconds |
| Data directory | `./data` | Sled database location |

## World Program

**Program ID:** `Wor1dProgram1111111111111111111111111111111`

**Instructions:**
- `InitializeWorld` - Create a new game world
- `JoinWorld` - Create player account in world
- `PlayerMovement` - Update velocity based on input
- `LeaveWorld` - Remove player from world

**PDAs:**
- World: `seeds = ["world", world_name]`
- Player: `seeds = ["world_player", world_pubkey, authority_pubkey]`

## Tech Stack

**Server (Rust):**
- `solana-svm` 2.1.0 - Transaction execution
- `solana-sdk` 2.1.0 - Types and crypto
- `axum` - HTTP server
- `tokio` - Async runtime
- `sled` - Embedded database
- `dashmap` - Concurrent hashmap

**Client (TypeScript):**
- `three` - 3D rendering
- `@solana/web3.js` - Transaction building
- `borsh` - Binary serialization
- `vite` - Build tool

## License

MIT
