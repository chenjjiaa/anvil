# Anvil 架构指南（中文版）

## 目录

1. [项目概述](#项目概述)
2. [系统架构](#系统架构)
3. [核心模块详解](#核心模块详解)
   - [Gateway（订单网关）](#gateway订单网关)
   - [Matching Engine（撮合引擎）](#matching-engine撮合引擎)
   - [Settlement（结算服务）](#settlement结算服务)
   - [SDK（客户端库）](#sdk客户端库)
4. [模块间交互流程](#模块间交互流程)
5. [技术实现细节](#技术实现细节)

---

## 项目概述

Anvil 是一个**高性能、自托管的订单簿和撮合基础设施**，专为区块链交易系统设计。它采用**链下撮合 + 链上结算**的混合架构，在保证高性能的同时确保交易的链上可验证性。

### 核心特性

- **低延迟撮合**：链下撮合引擎，延迟 < 100μs（p99）
- **链上结算**：所有成交结果最终在链上验证和结算
- **无托管**：不托管用户资金，资金始终在链上
- **高性能**：支持 > 100k 订单/秒的吞吐量
- **可扩展**：模块化设计，支持多市场、多链

### 设计原则

1. **性能优先**：使用高性能组件（actix-web、DashMap、gRPC）
2. **确定性匹配**：价格-时间优先级，结果可重现
3. **无状态设计**：各服务可独立扩展
4. **链上可验证**：所有交易最终在链上结算

---

## 系统架构

### 整体架构图

```
┌─────────────┐
│   Client    │ (使用 SDK)
└──────┬──────┘
       │ HTTP/REST
       │ (签名认证)
       ▼
┌─────────────────────────────────────┐
│         Gateway (网关服务)          │
│  - HTTP Server (actix-web)         │
│  - 认证 & 准入控制                  │
│  - 订单路由                         │
└──────┬──────────────────────────────┘
       │ gRPC
       │ (订单提交)
       ▼
┌─────────────────────────────────────┐
│    Matching Engine (撮合引擎)       │
│  - 订单簿管理 (DashMap)             │
│  - 价格-时间优先级匹配              │
│  - 成交结果生成                     │
└──────┬──────────────────────────────┘
       │ gRPC
       │ (成交结果)
       ▼
┌─────────────────────────────────────┐
│      Settlement (结算服务)          │
│  - 交易验证                         │
│  - 链上交易构建                     │
│  - 区块链提交                       │
└──────┬──────────────────────────────┘
       │ RPC
       ▼
┌─────────────────────────────────────┐
│         Blockchain                  │
│    (Solana / Ethereum)              │
└─────────────────────────────────────┘
```

### 服务通信协议

- **Client ↔ Gateway**: HTTP/REST (JSON)
- **Gateway ↔ Matching**: gRPC (Protocol Buffers)
- **Matching ↔ Settlement**: gRPC (Protocol Buffers)
- **Settlement ↔ Blockchain**: JSON-RPC / WebSocket

---

## 核心模块详解

### Gateway（订单网关）

Gateway 是系统的入口点，负责接收客户端订单、进行认证和准入控制，然后将订单路由到相应的撮合引擎。

#### 主要组件

1. **HTTP Server** (`server.rs`)

   - 基于 `actix-web` 的高性能 HTTP 服务器
   - 支持多工作线程（默认 = CPU 核心数）
   - 提供 RESTful API 接口

2. **认证模块** (`auth.rs`)

   - **Ed25519 签名验证**：支持 Ed25519 椭圆曲线签名
   - **ECDSA 签名验证**：支持 ECDSA (secp256k1) 签名
   - 签名格式：支持紧凑格式（64 字节）和 DER 格式（65 字节）
   - 消息序列化：使用规范化的 JSON 序列化确保签名一致性

3. **准入控制** (`admission.rs`)

   - **速率限制**：使用 `governor` crate 实现令牌桶算法
   - **市场可用性检查**：验证市场是否开放交易
   - **余额验证**：检查用户是否有足够余额（占位实现）
   - **订单验证**：验证订单格式、价格、数量等

4. **路由模块** (`router.rs`)
   - **市场到撮合引擎映射**：维护市场标识符到撮合引擎端点的映射
   - **gRPC 客户端管理**：按需创建和复用 gRPC 客户端连接
   - **订单转换**：将 Gateway 的 `PlaceOrderRequest` 转换为撮合引擎的 `Order` 格式

#### 工作流程

```
1. 客户端发送订单请求 (HTTP POST /api/v1/orders)
   ↓
2. Gateway 接收请求
   ↓
3. 提取用户 ID（从签名或认证令牌）
   ↓
4. 检查速率限制
   ↓
5. 验证订单签名（如果提供）
   ↓
6. 准入控制验证（市场、余额等）
   ↓
7. 路由到对应的撮合引擎（通过 gRPC）
   ↓
8. 返回订单响应（订单 ID、状态等）
```

#### API 接口

- `POST /api/v1/orders` - 提交订单
- `GET /api/v1/orders/{order_id}` - 查询订单状态
- `DELETE /api/v1/orders/{order_id}` - 取消订单
- `GET /health` - 健康检查

---

### Matching Engine（撮合引擎）

Matching Engine 是系统的核心，负责维护订单簿并执行撮合逻辑。

#### 主要组件

1. **订单簿** (`orderbook.rs`)

   - **数据结构**：使用 `DashMap` 实现无锁并发访问
   - **价格层级**：每个价格层级（PriceLevel）维护一个订单队列
   - **买卖分离**：买单（bids）和卖单（asks）分别维护
   - **FIFO 队列**：同一价格下，订单按时间顺序排列（先进先出）

   ```rust
   pub struct OrderBook {
       market: String,
       bids: Arc<DashMap<u64, PriceLevel>>,  // 价格 -> 价格层级
       asks: Arc<DashMap<u64, PriceLevel>>,
   }

   pub struct PriceLevel {
       price: u64,
       orders: Vec<Order>,  // FIFO 队列
       total_size: u64,
   }
   ```

2. **撮合器** (`matcher.rs`)

   - **价格-时间优先级**：
     - **价格优先**：更好的价格优先成交
     - **时间优先**：相同价格下，更早的订单优先成交
   - **撮合算法**：
     - 买单：与卖单簿（asks）中价格最低的订单匹配
     - 卖单：与买单簿（bids）中价格最高的订单匹配
   - **部分成交**：支持订单部分成交，剩余部分进入订单簿

3. **gRPC 服务器** (`server.rs`)

   - 接收来自 Gateway 的订单提交请求
   - 调用撮合器进行撮合
   - 返回撮合结果（成交列表、订单状态等）

4. **gRPC 客户端** (`client.rs`)
   - 将撮合产生的成交结果发送到 Settlement 服务
   - 异步提交，不阻塞撮合流程

#### 撮合流程

```
1. 接收订单（来自 Gateway）
   ↓
2. 获取或创建对应市场的订单簿
   ↓
3. 循环撮合：
   a. 查找对手方最优价格
   b. 检查价格是否可成交
   c. 如果可成交：
      - 计算成交数量（取最小值）
      - 创建成交记录（Trade）
      - 更新或移除对手方订单
      - 更新剩余数量
   d. 如果不可成交：退出循环
   ↓
4. 如果订单未完全成交：
   - 将剩余部分加入订单簿
   ↓
5. 如果有成交：
   - 异步发送成交结果到 Settlement
   ↓
6. 返回撮合结果
```

#### 撮合示例

假设订单簿状态：

- **买单簿（Bids）**：
  - 50000: [buy_1(1), buy_2(2)] // 价格 50000，订单 buy_1 数量 1，buy_2 数量 2
- **卖单簿（Asks）**：
  - 50010: [sell_1(1)] // 价格 50010，订单 sell_1 数量 1

**场景 1：卖单进入**

- 新卖单：价格 50000，数量 1
- 匹配：与 buy_1 匹配（价格相同，时间优先）
- 结果：完全成交，buy_1 被移除

**场景 2：买单进入**

- 新买单：价格 50010，数量 2
- 匹配：与 sell_1 匹配（价格相同）
- 结果：sell_1 完全成交，买单部分成交（剩余 1 进入订单簿）

---

### Settlement（结算服务）

Settlement 服务负责验证成交结果、构建链上交易并提交到区块链。

#### 主要组件

1. **交易验证器** (`validator.rs`)

   - **价格限制检查**：验证成交价格在合理范围内
   - **市场状态检查**：验证市场是否允许结算
   - **重放保护**：防止重复提交相同的交易

2. **交易构建器** (`transaction.rs`)

   - **链抽象**：提供统一的交易构建接口
   - **链特定实现**：
     - Solana：构建 Solana 交易
     - Ethereum：构建 Ethereum 交易
   - **批量处理**：支持将多个成交打包到一个交易中

3. **提交器** (`submitter.rs`)

   - **交易提交**：将构建好的交易提交到区块链
   - **状态跟踪**：跟踪交易状态（Pending → Submitted → Confirmed）
   - **确认数跟踪**：跟踪交易确认数
   - **错误处理**：处理提交失败和交易回滚

4. **链特定实现** (`chains/`)
   - **Solana** (`solana.rs`)：使用 Solana SDK 构建和提交交易
   - **Ethereum** (`ethereum.rs`)：使用 Ethereum SDK 构建和提交交易

#### 结算流程

```
1. 接收成交结果（来自 Matching Engine）
   ↓
2. 验证成交结果
   - 价格检查
   - 市场状态检查
   - 重放保护
   ↓
3. 选择目标链（Solana / Ethereum）
   ↓
4. 构建链上交易
   - 调用链特定的交易构建器
   - 将成交结果编码为链上交易
   ↓
5. 提交交易到区块链
   - 通过 RPC 端点提交
   - 获取交易哈希
   ↓
6. 跟踪交易状态
   - 轮询区块链状态
   - 更新确认数
   ↓
7. 返回结算结果
```

---

### SDK（客户端库）

SDK 提供客户端库，方便开发者集成 Anvil 系统。

#### 主要功能

1. **HTTP 客户端** (`client.rs`)

   - 基于 `reqwest` 的异步 HTTP 客户端
   - 提供订单提交、查询、取消接口
   - 自动处理序列化/反序列化

2. **签名工具** (`signing.rs`)

   - **Ed25519 签名**：生成和验证 Ed25519 签名
   - **ECDSA 签名**：生成和验证 ECDSA 签名
   - **密钥生成**：提供密钥对生成工具
   - **消息序列化**：规范化消息序列化确保签名一致性

3. **类型定义** (`types.rs`)
   - 订单类型（Order、PlaceOrderRequest 等）
   - 成交类型（Trade）
   - 枚举类型（Side、OrderType、OrderStatus 等）

#### 使用示例

```rust
use anvil_sdk::{Client, SignatureAlgorithm, PlaceOrderRequest, Side, OrderType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建客户端
    let client = Client::new("http://localhost:8080");

    // 创建订单请求
    let request = PlaceOrderRequest {
        market: "BTC-USDT".to_string(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: Some(50000),
        size: 1,
        client_order_id: Some("my_order_1".to_string()),
        signature: "".to_string(),
    };

    // 签名并提交订单
    let private_key = b"your_private_key_here";
    let response = client
        .place_order_signed(request, private_key, SignatureAlgorithm::Ed25519)
        .await?;

    println!("订单已提交: {}", response.order_id);
    Ok(())
}
```

---

## 模块间交互流程

### 完整订单流程

```
┌─────────┐
│ Client  │
└────┬────┘
     │ 1. HTTP POST /api/v1/orders
     │    { market, side, price, size, signature }
     ▼
┌─────────────────┐
│    Gateway      │
│  - 验证签名      │
│  - 准入控制      │
│  - 路由订单      │
└────┬────────────┘
     │ 2. gRPC SubmitOrder
     │    { order_id, market, side, price, size }
     ▼
┌─────────────────┐
│ Matching Engine │
│  - 撮合订单      │
│  - 更新订单簿    │
│  - 生成成交      │
└────┬────────────┘
     │ 3a. 返回撮合结果
     │     { order_id, status, trades }
     │
     │ 3b. gRPC SubmitTrades (异步)
     │     { market, trades[], chain }
     ▼
┌─────────────────┐
│   Settlement    │
│  - 验证成交      │
│  - 构建交易      │
│  - 提交链上      │
└────┬────────────┘
     │ 4. 返回交易哈希
     │    { tx_hash, status }
     ▼
┌─────────────────┐
│   Blockchain    │
└─────────────────┘
```

### 详细交互时序

#### 1. 订单提交流程

```
Client                    Gateway                  Matching              Settlement
  │                         │                         │                      │
  │─── POST /orders ───────>│                         │                      │
  │                         │                         │                      │
  │                         │─── 验证签名 ────────────│                      │
  │                         │─── 准入控制 ────────────│                      │
  │                         │                         │                      │
  │                         │─── gRPC SubmitOrder ────>│                      │
  │                         │                         │                      │
  │                         │                         │─── 撮合订单 ────────│
  │                         │                         │─── 更新订单簿 ──────│
  │                         │                         │─── 生成成交 ────────│
  │                         │                         │                      │
  │                         │<── SubmitOrderResponse ──│                      │
  │                         │    { order_id, status }  │                      │
  │<── 200 OK ──────────────│                         │                      │
  │    { order_id }         │                         │                      │
  │                         │                         │                      │
  │                         │                         │─── SubmitTrades ────>│
  │                         │                         │    (异步)             │
  │                         │                         │                      │
  │                         │                         │                      │─── 验证成交
  │                         │                         │                      │─── 构建交易
  │                         │                         │                      │─── 提交链上
  │                         │                         │<── SubmitResponse ────│
  │                         │                         │    { tx_hash }        │
```

#### 2. 撮合引擎内部流程

```
接收订单
  │
  ├─> 获取订单簿（按市场）
  │
  ├─> 循环撮合：
  │     │
  │     ├─> 查找对手方最优价格
  │     │     - 买单：查找最低卖价
  │     │     - 卖单：查找最高买价
  │     │
  │     ├─> 检查价格是否可成交
  │     │     - 买单：order.price >= best_ask
  │     │     - 卖单：order.price <= best_bid
  │     │
  │     ├─> 如果可成交：
  │     │     │
  │     │     ├─> 获取对手方订单（FIFO）
  │     │     │
  │     │     ├─> 计算成交数量
  │     │     │     match_size = min(remaining_size, maker.remaining_size)
  │     │     │
  │     │     ├─> 创建成交记录
  │     │     │     Trade {
  │     │     │       price: match_price,
  │     │     │       size: match_size,
  │     │     │       maker_order_id,
  │     │     │       taker_order_id,
  │     │     │     }
  │     │     │
  │     │     ├─> 更新对手方订单
  │     │     │     - 完全成交：移除订单
  │     │     │     - 部分成交：更新剩余数量
  │     │     │
  │     │     └─> 更新剩余数量
  │     │           remaining_size -= match_size
  │     │
  │     └─> 如果不可成交：退出循环
  │
  ├─> 如果订单未完全成交：
  │     └─> 将剩余部分加入订单簿
  │
  └─> 返回撮合结果
        MatchResult {
          order,
          trades: Vec<Trade>,
          fully_filled: bool,
          partially_filled: bool,
        }
```

---

## Proto 文件生成说明

### Proto 文件位置

项目中的 proto 文件定义在以下位置：

- `crates/matching/proto/matching.proto` - 撮合引擎的 gRPC 服务定义
- `crates/settlement/proto/settlement.proto` - 结算服务的 gRPC 服务定义

### 自动生成机制

Proto 文件会在编译时通过 `build.rs` 脚本自动生成 Rust 代码：

1. **编译时生成**：运行 `cargo build` 时，`tonic-build` 会自动编译 proto 文件
2. **生成位置**：生成的代码位于 `target/debug/build/{crate-name}-{hash}/out/` 目录
3. **代码包含**：通过 `tonic::include_proto!` 宏在运行时包含生成的代码

### 各服务的 Proto 使用

**Matching Engine**：

- 生成：`anvil.matching.rs`（服务器和客户端代码）
- 使用：`tonic::include_proto!("anvil.matching")`

**Settlement**：

- 生成：`anvil.settlement.rs`（服务器和客户端代码）
- 使用：`tonic::include_proto!("anvil.settlement")`

**Gateway**：

- 不生成自己的 proto 文件
- 作为客户端使用 Matching 的 proto（通过 `build.rs` 编译 `matching.proto`）

### 生成的文件示例

编译后会在 `target/debug/build/` 目录下生成：

```
target/debug/build/
├── anvil-matching-{hash}/
│   └── out/
│       ├── anvil.matching.rs      # Matching 服务代码
│       └── anvil.settlement.rs    # Settlement 客户端代码
├── anvil-settlement-{hash}/
│   └── out/
│       └── anvil.settlement.rs    # Settlement 服务代码
└── anvil-gateway-{hash}/
    └── out/
        └── anvil.matching.rs      # Matching 客户端代码
```

### 注意事项

1. **无需手动生成**：proto 文件会在编译时自动生成，无需手动操作
2. **生成的文件不需要提交**：`target/` 目录已在 `.gitignore` 中，生成的文件不会提交到版本控制
3. **修改 proto 后重新编译**：修改 proto 文件后，需要重新运行 `cargo build` 来重新生成代码
4. **protoc 依赖**：确保已安装 `protoc` 编译器（见 README 中的前置要求）
5. **Proto 文件是手动维护的**：Proto 文件是手动定义的，不是从 Rust struct 自动生成的。如果修改了 Rust struct，需要手动同步更新对应的 proto 文件
6. **Gateway 不需要 proto 文件**：Gateway 只作为客户端使用 Matching 的 proto，不需要定义自己的 proto 文件

### 更新 Proto 文件

当需要更新 proto 文件时，可以使用以下命令：

```bash
# 强制重新生成 proto 代码
just proto

# 或者只验证 proto 文件是否能正确编译
just proto-check
```

**重要提示**：

- Proto 文件是**手动维护**的，不会自动从 Rust struct 生成
- 如果修改了 Rust 类型定义，需要**手动同步**更新对应的 proto 文件
- 修改 proto 文件后，运行 `just proto` 或 `cargo build` 来重新生成 Rust 代码

---

## 技术实现细节

### 并发模型

1. **Gateway**

   - **多线程模型**：actix-web 使用多工作线程处理请求
   - **异步 I/O**：使用 Tokio 异步运行时
   - **连接池**：gRPC 客户端连接复用

2. **Matching Engine**

   - **无锁并发**：使用 `DashMap` 实现无锁订单簿访问
   - **读写分离**：订单簿支持并发读写
   - **异步处理**：gRPC 服务器使用异步处理

3. **Settlement**
   - **异步提交**：交易提交使用异步 I/O
   - **状态跟踪**：使用 `RwLock` 保护共享状态

### 性能优化

1. **订单簿优化**

   - 使用 `DashMap` 替代 `BTreeMap` 实现无锁并发
   - 价格层级使用 `Vec` 实现 FIFO 队列
   - 避免不必要的克隆和分配

2. **网络优化**

   - 使用 gRPC 进行服务间通信（二进制协议）
   - 连接复用减少连接开销
   - 批量处理减少网络往返

3. **内存优化**
   - 使用 `Arc` 共享不可变数据
   - 及时释放已成交订单
   - 避免内存泄漏

### 错误处理

1. **分层错误处理**

   - Gateway：`GatewayError`（认证、准入、路由错误）
   - Matching：`MatchingError`（撮合错误）
   - Settlement：`SubmissionError`（提交错误）

2. **错误传播**
   - 使用 `thiserror` 进行错误定义
   - 使用 `?` 操作符传播错误
   - 在边界处转换错误类型

### 配置管理

1. **环境变量**

   - 服务地址、端口
   - RPC 端点
   - 速率限制参数

2. **配置文件**
   - 支持 TOML/YAML 格式
   - 使用 `config` crate 加载配置
   - 环境变量覆盖配置文件

### 日志和监控

1. **结构化日志**

   - 使用 `tracing` 进行日志记录
   - 支持日志级别过滤
   - 结构化字段便于查询

2. **指标收集**
   - 请求延迟
   - 撮合延迟
   - 吞吐量
   - 错误率

---

## 总结

Anvil 是一个高性能、模块化的交易基础设施系统，采用**链下撮合 + 链上结算**的混合架构。通过 Gateway、Matching Engine 和 Settlement 三个核心服务的协作，实现了低延迟、高吞吐量的交易处理能力，同时保证了交易的链上可验证性。

系统的设计充分考虑了性能、可扩展性和可靠性，使用现代化的 Rust 技术栈（actix-web、DashMap、gRPC）实现了高性能的并发处理能力。
