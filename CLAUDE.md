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

# Run tests
cargo test -p l2-runtime              # All runtime tests
cargo test -p l2-runtime join_world   # JoinWorld integration tests

# Client development
cd client
npm install
npm run dev      # Development server with hot reload (http://localhost:3000)
npm run build    # Production build
```

## Architecture

### Crate Structure

```
crates/
├── validator/       # Main binary entry point, CLI args, node orchestration
├── l2-runtime/      # Core SVM execution engine
│   ├── processor.rs     # TransactionBatchProcessor wrapper
│   ├── callback.rs      # L2AccountLoader - SVM account bridge (critical for PDA ownership)
│   ├── block_producer.rs # 30Hz game loop
│   ├── account_store.rs  # DashMap-based in-memory storage
│   ├── persistence.rs    # Sled KV store for disk persistence
│   └── tests/           # Integration tests for JoinWorld flow
│       └── join_world_test.rs
├── rpc-server/      # JSON-RPC over HTTP (8899) and WebSocket (8900)
├── world-program/   # Game logic as SVM builtin (movement, combat, physics)
│   ├── builtin.rs       # Native instruction processor (not BPF)
│   ├── instruction.rs   # WorldInstruction enum
│   ├── state.rs         # WorldConfig, WorldPlayer structs
│   └── constants.rs     # Seeds, default values
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
- **On-the-fly PDA creation**: Missing PDAs are auto-created with `world_program` as owner (see Account Ownership below)
- **Fire-and-forget transactions**: Movement sends return immediately without confirmation
- **World program as builtin**: Registered directly with SVM, not as deployed BPF program

### Account Ownership (Critical)

The `L2AccountLoader` callback creates accounts on-the-fly for missing pubkeys:

- **Wallets (on-curve)**: Created with `system_program` as owner, 0 bytes data
- **PDAs (off-curve)**: Created with `world_program` as owner, `WorldPlayer::LEN` bytes data

This is critical because the SVM enforces that programs can only write to accounts they own. If PDAs were owned by `system_program`, the world program would fail with `InvalidAccountData` when trying to write player state.

See `crates/l2-runtime/src/callback.rs` - `get_account_shared_data()` method.

### World Program

- **Program ID**: `Wor1dProgram1111111111111111111111111111111`
- **PDAs**: World = `[b"world", name]`, Player = `[b"world_player", world, authority]`
- **Physics**: Fixed-point (1000 = 1.0), gravity -30/tick², sprint 500 units/tick
- **State**: WorldConfig (118 bytes), WorldPlayer (123 bytes with 3D position/velocity)

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

## SVM Integration Details

### Transaction Processing Architecture

The L2 uses Solana's `TransactionBatchProcessor` from `solana-svm` crate to execute transactions.

**Key files:**
- `crates/l2-runtime/src/processor.rs` - L2Processor wraps TransactionBatchProcessor
- `crates/l2-runtime/src/callback.rs` - L2AccountLoader implements TransactionProcessingCallback
- `crates/l2-runtime/src/block_producer.rs` - 30Hz game loop, calls process_transactions()

**Transaction execution flow:**
```
Client HTTP POST -> RPC Server -> Transaction Queue (bounded 1024)
    -> Block Producer (30Hz, max 64 txs/block)
    -> L2Processor.process_transactions()
        -> L2AccountLoader callbacks (get_account_shared_data, account_matches_owners)
        -> TransactionBatchProcessor.load_and_execute_sanitized_transactions()
        -> World Program builtin execution
    -> Account Store updates -> Persistence (every 300 slots)
```

### Program Cache & Fork Graph

The SVM uses a program cache with slot-based visibility:
- `L2ForkGraph` implements `ForkGraph` trait for slot relationship queries
- Builtins registered at slot 0 are visible at all future slots (Ancestor relationship)
- The fork graph is a simple linear chain (no actual forks in L2)

**Critical configuration in L2Processor::new():**
1. Create `TransactionBatchProcessor::new_uninitialized(slot=0, epoch=0)`
2. Set fork graph: `program_cache.set_fork_graph(Arc::downgrade(&fork_graph))`
3. Initialize runtime environments v1 and v2 (required for syscalls)
4. Register builtins at slot 0 (system_program, bpf_loaders, world_program)

### Debugging Transaction Hangs

If transactions hang during SVM execution:

**Symptoms:**
- Client shows "Joining world..." indefinitely
- Server logs show account loading (get_account_shared_data) works
- No `[BUILTIN]` eprintln messages appear (builtin never invoked)
- Hang occurs AFTER account loading, BEFORE builtin execution

**Common causes:**
1. **Program cache issues** - Builtins not visible at processing slot
2. **Slot mismatch** - Processor slot vs fork graph slot vs transaction slot
3. **Missing runtime environments** - `create_program_runtime_environment_v1/v2` not called

**What NOT to do:**
- Don't call `processor.new_from()` every slot - creates fresh cache, expensive
- Don't re-register builtins every slot - wastes CPU, can cause cache issues

**Correct pattern:**
- Register builtins ONCE at startup with deployment_slot=0
- Keep processor stable across slots
- Only update per-slot: fork graph slot, clock sysvar, blockhash

### Builtin Tracing

The world-program builtin has `eprintln!("[BUILTIN]...")` calls that bypass SVM log collection:
- `[BUILTIN] process_instruction_inner ENTRY` - Builtin was invoked
- `[BUILTIN] process_join_world ENTRY` - JoinWorld instruction dispatched
- `[BUILTIN] process_join_world SUCCESS` - Join completed

If these don't appear, the hang is in SVM program resolution, not in the builtin itself.

## Test Suite

The `l2-runtime` crate has comprehensive tests for the JoinWorld flow:

**Location:** `crates/l2-runtime/src/tests/join_world_test.rs`

**Test cases:**
- `test_processor_initialization` - Verifies builtins and sysvars are registered
- `test_world_account_creation` - Tests InitializeWorld transaction
- `test_join_world_transaction_processing` - Tests JoinWorld creates player account
- `test_account_loader_callback` - Tests on-the-fly account creation with correct ownership
- `test_block_producer_processes_join` - Tests transaction queue submission
- `test_end_to_end_join_flow` - Full client simulation
- `test_multiple_players_join` - Tests multiple players in same world
- `test_processor_slot_advancement` - Tests slot/blockhash updates

**Running tests:**
```bash
cargo test -p l2-runtime join_world  # Run JoinWorld tests
cargo test -p l2-runtime             # Run all l2-runtime tests
```

## Common Issues

### "Join failed" / Player stuck on "Joining world..."

**Symptom:** Client shows join failed, server logs show builtin execution stops at `get_data_mut()`

**Cause:** PDAs created with wrong owner. The SVM only allows programs to write to accounts they own.

**Fix:** Ensure `L2AccountLoader::get_account_shared_data()` returns PDAs owned by `world_program::id()`, not `system_program::id()`.

### Transaction succeeds but account not created

**Symptom:** SVM returns success but `getAccountInfo` returns null

**Cause:** Account modifications not being written back to AccountStore after execution.

**Fix:** Check `L2Processor::process_transactions()` properly extracts and stores modified accounts from `TransactionBatchProcessor` results.
