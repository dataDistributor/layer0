# Architecture

## Overview
dxid is a Layer-0 identity fabric with hybrid PoW+PoS consensus, zk-STARK-based external chain handshakes, Groth16 zk-SNARK proofs for cross-chain messaging, and vector-native storage on Postgres+pgvector. The workspace is fully modular to allow swapping crypto, consensus, storage, and interop adapters.

## Crate responsibilities
- `dxid-core`: Domain types (`Block`, `Transaction`, `Identity`, `CrossChainMessage`, `ChainMetadata`), execution engine, tokenomics (max supply + dynamic halving), identity flows (create/add-attr/rotate/revoke), OAuth-like proof helpers.
- `dxid-crypto`: Blake3 hashing, ed25519 signatures, `CryptoProvider` impl, STARK backend (Winterfell Fibonacci demo), SNARK backend (Groth16 sum circuit demo), address encoding helpers.
- `dxid-consensus`: Hybrid PoW + PoS. PoW uses double-blake3 difficulty target; PoS selects validators by stake weight. Includes staking/unstaking/slashing and block validation.
- `dxid-storage`: Postgres + pgvector-backed stores for blocks, balances, identities, embeddings; traits for block/state/identity/vector storage.
- `dxid-vectors`: Embedding helpers and data model (`Embedding`, `EmbeddingId`), identity and chain-state embedding builders.
- `dxid-network`: libp2p gossip + mdns + identify; block/tx broadcasting and peer discovery.
- `dxid-interop`: Generic HTTP/JSON-RPC adapter with zk-STARK connectivity check and zk-SNARK message proofing; configurable external chain metadata.
- `dxid-config`: Typed configuration loader (TOML + env override).
- `dxid-rpc`: REST (axum) and gRPC (tonic) services exposing health, blocks, balances, AI queries.
- `dxid-wallet`: Wallet store with bip39 mnemonic generation, encrypted secret storage (PBKDF2 + AES-GCM), address derivation helpers.
- `dxid-contracts`: Contract trait and registry plus a KV example for future WASM runtime.
- `dxid-ai-hypervisor`: OpenAI client to answer operator questions using chain context.
- `dxid-node`: Node wiring: load config, init logging, connect Postgres, build consensus/network/rpc/ai services, start servers.
- `dxid-cli`: CLI driver; defaults to launching TUI when no subcommand; supports init/node/wallet/ai subcommands.
- `dxid-tui`: Terminal UI with tabs (Dashboard, Wallet, Identities, Chains, Bridge, Mining, AI) and AI chat pane.

## Data flow
1. **Transactions** -> broadcast via libp2p -> validated by consensus (signatures via `CryptoProvider`) -> executed by `dxid-core::ExecutionEngine` -> persisted via `dxid-storage` (blocks, balances, identities, vectors).
2. **Identity updates** -> validated (status, key ownership) -> stored in `identities` table -> optional embeddings inserted via `dxid-vectors` -> discoverable through RPC/CLI/TUI.
3. **Cross-chain messages** -> proven with Groth16 backend (`dxid-crypto`) -> sent via `dxid-interop` HTTP adapter -> receipts returned to RPC/CLI.
4. **Network** -> libp2p gossip handles blocks/txs; mdns for local discovery; configurable seeds.
5. **AI hypervisor** -> collects summary (height/peers/embedding hints) -> queries OpenAI -> results available via REST/gRPC/CLI/TUI.

## Consensus specifics
- **PoW**: hashes block header (double blake3) with nonce until `< target`. Difficulty tracked in `ConsensusState`; target derived inversely from difficulty.
- **PoS**: validators stake DXID; validator chosen by stake weight among eligible peers; slashing helper included.
- **Rewards**: Execution engine computes reward using dynamic halving (height- and supply-driven) and enforces max supply cap with treasury split.

## Storage schema
- `blocks(height bigint primary key, data jsonb)`
- `balances(address bytea primary key, amount bigint)`
- `identities(id uuid primary key, data jsonb)`
- `embeddings(id text primary key, namespace text, vector vector(1536), metadata jsonb)`

## APIs
- REST: `/health`, `/status`, `/blocks/{height}`, `/balance/{address}`, `/ai/query` (extendable to identities, chains, mining).
- gRPC: `Dxid` service in `dxid-rpc/proto/dxid.proto` with status/block/balance/ai methods.

## Deployment
- Single-process node (network + consensus + storage + rpc + ai).
- Dockerfile builds `dxid-node` release binary. Railway deployment notes in `docs/deployment_railway.md`.
