# Anvil

<div align="left">

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

</div>

A self-hosted, high-performance orderbook and matching infrastructure that enables protocols to combine off-chain speed with on-chain security—without custody or trust assumptions.

## Overview

Anvil is an **infrastructure toolkit** for building professional-grade trading systems. It provides:

- Low-latency, deterministic off-chain matching
- On-chain, verifiable settlement
- Clear trust and responsibility boundaries

**Anvil is not a DEX**, **not a hosted service**, and **does not custody user funds**. It is infrastructure that protocols can deploy and operate themselves.

## Architecture

Anvil follows a hybrid architecture:

> **Off-chain matching + On-chain settlement**

### Components

- **Gateway** (`anvil-gateway`): Order intake, authentication, and validation
- **Matching Engine** (`anvil-matching`): High-performance limit order book matching
- **Settlement Core** (`anvil-settlement`): Trade validation and blockchain transaction submission
- **SDK** (`anvil-sdk`): Client library for order submission

## Project Structure

```
anvil/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── gateway/            # Order gateway service
│   ├── matching/           # Matching engine service
│   ├── settlement/         # Settlement core service
│   └── sdk/                # Client SDK (library-only)
├── deploy/                 # Helm charts and deployment configs
└── docs/                   # Design documentation
```

## Building

Build all crates:

```bash
cargo build
```

Build individual services:

```bash
cargo build -p anvil-gateway
cargo build -p anvil-matching
cargo build -p anvil-settlement
```

## Running

Run individual services:

```bash
cargo run -p anvil-gateway
cargo run -p anvil-matching
cargo run -p anvil-settlement
```

## Documentation

- [Architecture Guide](docs/architecture.md) - Engineering structure and conventions
- [Design and Implementation](docs/design-and-implementation.md) - System design and usage

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
