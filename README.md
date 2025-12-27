# Anvil

<div align="left">

[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)

</div>

A self-hosted, high-performance orderbook and matching infrastructure that enables protocols to combine off-chain speed with on-chain security—without custody or trust assumptions

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

  - High-performance HTTP server using actix-web
  - Ed25519/ECDSA signature verification
  - Rate limiting and admission control
  - gRPC client for matching engine communication

- **Matching Engine** (`anvil-matching`): High-performance limit order book matching

  - Deterministic price-time priority matching
  - Concurrent order book using DashMap
  - gRPC server for order processing
  - gRPC client for settlement communication

- **Settlement Core** (`anvil-settlement`): Trade validation and blockchain transaction submission

  - Trade validation and protocol rule enforcement
  - Chain-specific transaction building (Solana/Ethereum)
  - Transaction submission and confirmation tracking
  - gRPC server for receiving matched trades

- **SDK** (`anvil-sdk`): Client library for order submission
  - Async HTTP client using reqwest
  - Ed25519/ECDSA signing support
  - Synchronous and asynchronous interfaces

## Project Structure

```
anvil/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── gateway/            # Order gateway service
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── server.rs          # actix-web HTTP server
│   │   │   ├── handlers.rs        # HTTP request handlers
│   │   │   ├── auth.rs            # Authentication (Ed25519/ECDSA)
│   │   │   ├── admission.rs       # Rate limiting & admission control
│   │   │   ├── router.rs          # Order routing
│   │   │   ├── grpc_client.rs     # gRPC client for matching
│   │   │   ├── config.rs          # Configuration
│   │   │   └── middleware.rs      # HTTP middleware
│   │   └── proto/                 # gRPC proto files
│   ├── matching/           # Matching engine service
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── lib.rs
│   │   │   ├── server.rs          # gRPC server
│   │   │   ├── matcher.rs         # Matching logic
│   │   │   ├── orderbook.rs       # Order book (DashMap-based)
│   │   │   ├── client.rs          # gRPC client for settlement
│   │   │   ├── types.rs
│   │   │   └── config.rs
│   │   └── proto/                 # gRPC proto files
│   ├── settlement/         # Settlement core service
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── server.rs          # gRPC server
│   │   │   ├── validator.rs       # Trade validation
│   │   │   ├── transaction.rs    # Transaction building
│   │   │   ├── submitter.rs      # Transaction submission
│   │   │   ├── config.rs
│   │   │   └── chains/           # Chain-specific implementations
│   │   │       ├── mod.rs
│   │   │       ├── solana.rs
│   │   │       └── ethereum.rs
│   │   └── proto/                 # gRPC proto files
│   └── sdk/                # Client SDK (library-only)
│       └── src/
│           ├── lib.rs
│           ├── client.rs          # HTTP client
│           ├── signing.rs        # Signing utilities
│           └── types.rs
├── deploy/                 # Helm charts and deployment configs
└── docs/                   # Design documentation
```

## Building

### Prerequisites

- Rust 1.92.0 or later
- protoc (Protocol Buffers compiler)

  ```bash
  # macOS
  brew install protobuf

  # Linux
  sudo apt-get install protobuf-compiler
  ```

### Build Commands

Build all crates:

```bash
cargo build --release
```

Build individual services:

```bash
cargo build --release -p anvil-gateway
cargo build --release -p anvil-matching
cargo build --release -p anvil-settlement
```

## Running

### Configuration

Services can be configured via environment variables or configuration files:

**Gateway:**

- `GATEWAY_BIND_ADDR`: HTTP server bind address (default: `0.0.0.0:8080`)
- `GATEWAY_WORKERS`: Number of worker threads (default: CPU count)
- `GATEWAY_MATCHING_ENGINES`: JSON mapping of market to matching engine endpoint

**Matching:**

- `MATCHING_ADDR`: gRPC server bind address (default: `0.0.0.0:50051`)
- `MARKET`: Market identifier (default: `BTC-USDT`)
- `MATCHING_SETTLEMENT_ENDPOINT`: Settlement service endpoint

**Settlement:**

- `SETTLEMENT_ADDR`: gRPC server bind address (default: `0.0.0.0:50052`)
- `SETTLEMENT_RPC_ENDPOINTS`: JSON mapping of chain to RPC endpoint

### Run Services

```bash
# Terminal 1: Start Settlement
cargo run --release -p anvil-settlement

# Terminal 2: Start Matching Engine
MARKET=BTC-USDT cargo run --release -p anvil-matching

# Terminal 3: Start Gateway
cargo run --release -p anvil-gateway
```

## Usage Example

### Using the SDK

```rust
use anvil_sdk::{Client, SignatureAlgorithm, PlaceOrderRequest, Side, OrderType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client
    let client = Client::new("http://localhost:8080");

    // Create order request
    let request = PlaceOrderRequest {
        market: "BTC-USDT".to_string(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: Some(50000),
        size: 1,
        client_order_id: Some("my_order_1".to_string()),
        signature: "".to_string(), // Will be signed automatically
    };

    // Sign and place order
    let private_key = b"your_private_key_here";
    let response = client
        .place_order_signed(request, private_key, SignatureAlgorithm::Ed25519)
        .await?;

    println!("Order placed: {}", response.order_id);

    // Query order status
    let order = client.get_order(&response.order_id).await?;
    println!("Order status: {:?}", order.status);

    Ok(())
}
```

## Performance

Anvil is designed for high-performance trading:

- **HTTP Latency**: < 1ms (p99) with actix-web
- **Matching Latency**: < 100μs (p99) with optimized order book
- **Throughput**: > 100k orders/sec per matching engine
- **Concurrency**: Lock-free order book using DashMap

## Development

### Code Formatting

```bash
just fmt
```

### Linting

```bash
just lint
```

### Testing

```bash
cargo test --workspace
```

## Documentation

- [Architecture Guide](docs/architecture.md) - Engineering structure and conventions
- [Design and Implementation](docs/design-and-implementation.md) - System design and usage

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
