# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SolanaL2 is a high-performance Layer 2 gaming chain built on the Solana Virtual Machine (SVM). It runs a 30Hz game loop for real-time multiplayer gameplay with gasless transactions and on-chain game state.

## Build Commands

```bash
# Build the validator (Rust)
cargo build --release

# Run the validator (leader mode)
cargo run --release --bin solana-l2

# Run in validator mode
cargo run --release --bin solana-l2 -- --mode validator

# Client development
cd client
npm install
npm run dev      # Development server with hot reload
npm run build    # Production build
```

## Architecture

### Crate Structure

```
crates/
├── validator/       # Main binary entry point, CLI args, node orchestration
├── l2-runtime/      # Core SVM execution engine
│   ├── processor.rs     # TransactionBatchProcessor wrapper
│   ├── callback.rs      # L2AccountLoader - SVM account bridge
│   ├── block_producer.rs # 30Hz game loop
│   ├── account_store.rs  # DashMap-based in-memory storage
│   └── persistence.rs    # Sled KV store for disk persistence
├── rpc-server/      # JSON-RPC over HTTP (8899) and WebSocket (8900)
├── world-program/   # Game logic as SVM builtin (movement, combat, physics)
├── l2-consensus/    # Leader broadcasts state to validators
└── l1-bridge/       # Future L1 settlement integration
```

### Transaction Flow

```
Client HTTP POST -> RPC Server -> Transaction Queue (bounded 1024)
    -> Block Producer (30Hz, max 64 txs/block)
    -> L2Processor (SVM execution)
    -> Account Store -> Persistence (every 300 slots)
```

### Key Design Decisions

- **30Hz tick rate**: Block time is 33ms for real-time gameplay
- **Gasless**: `lamports_per_signature = 0`, no fee validation
- **On-the-fly accounts**: Missing accounts return 256-byte zeroed account, enabling PDA creation without separate `create_account`
- **Fire-and-forget transactions**: Movement sends return immediately without confirmation
- **World program as builtin**: Registered directly with SVM, not as deployed BPF program

### World Program

- **Program ID**: `Wor1dProgram1111111111111111111111111111111`
- **PDAs**: World = `[b"world", name]`, Player = `[b"world_player", world, authority]`
- **Physics**: Fixed-point (1000 = 1.0), gravity -30/tick², sprint 500 units/tick
- **State**: WorldConfig (123 bytes), WorldPlayer (123 bytes with 3D position/velocity)

### Client (TypeScript)

- Three.js 3D renderer with third-person camera
- `L2Connection` handles HTTP RPC + WebSocket subscriptions
- `GameClient` builds transactions with proper PDA derivation
- 30Hz input loop matches server tick rate

## Key Constants

```rust
// l2-runtime/src/lib.rs
BLOCK_TIME_MS: 33        // 30Hz
MAX_TXS_PER_BLOCK: 64
TICKS_PER_SECOND: 30

// world-program/src/lib.rs
DEFAULT_PLAYER_HEALTH: 100
SPRINT_SPEED: 500
NORMAL_SPEED: 250
GRAVITY: -30
JUMP_VELOCITY: 400
```

## Dependencies

- **Solana SVM**: `solana-svm`, `solana-sdk`, `solana-program` (pinned to 2.1.0)
- **Storage**: `sled` (embedded KV), `dashmap` (concurrent hashmap)
- **Networking**: `axum`, `tokio-tungstenite`, `jsonrpsee`
- **Client**: `@solana/web3.js`, `three`, `borsh`

## Git Guidelines

Do not commit these directories (local data/build artifacts):
- `data/` - Sled database and chain state
- `client/dist/` - Built client files
