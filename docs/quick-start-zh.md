# Anvil 快速入门指南（中文版）

## 目录

1. [项目简介](#项目简介)
2. [快速开始](#快速开始)
3. [核心概念](#核心概念)
4. [使用示例](#使用示例)
5. [常见问题](#常见问题)

---

## 项目简介

Anvil 是一个**高性能、自托管的订单簿和撮合基础设施**，专为区块链交易系统设计。

### 核心特性

- ✅ **低延迟撮合**：链下撮合引擎，延迟 < 100μs
- ✅ **链上结算**：所有成交结果最终在链上验证
- ✅ **无托管**：不托管用户资金
- ✅ **高性能**：支持 > 100k 订单/秒
- ✅ **可扩展**：模块化设计，支持多市场、多链

### 架构概述

```
Client → Gateway → Matching Engine → Settlement → Blockchain
```

- **Gateway**：订单入口，认证和路由
- **Matching Engine**：订单簿和撮合逻辑
- **Settlement**：链上交易构建和提交

---

## 快速开始

### 前置要求

1. **Rust 环境**

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **protoc 编译器**

   ```bash
   # macOS
   brew install protobuf

   # Linux
   sudo apt-get install protobuf-compiler
   ```

### 构建项目

```bash
# 克隆仓库
git clone <repository-url>
cd anvil

# 构建所有服务
cargo build --release

# 或构建单个服务
cargo build --release -p anvil-gateway
cargo build --release -p anvil-matching
cargo build --release -p anvil-settlement
```

### 运行服务

**终端 1：启动 Settlement 服务**

```bash
cargo run --release -p anvil-settlement
```

**终端 2：启动 Matching Engine**

```bash
MARKET=BTC-USDT cargo run --release -p anvil-matching
```

**终端 3：启动 Gateway**

```bash
cargo run --release -p anvil-gateway
```

### 验证服务

```bash
# 检查 Gateway 健康状态
curl http://localhost:8080/health

# 应该返回：
# {"status":"ok","service":"anvil-gateway"}
```

---

## 核心概念

### 1. 订单类型

**限价单（Limit Order）**

- 指定价格和数量
- 只有当价格可成交时才成交
- 未成交部分进入订单簿

**市价单（Market Order）**

- 只指定数量，不指定价格
- 立即以最优价格成交
- 可能产生滑点

### 2. 订单方向

- **买单（Buy）**：买入资产
- **卖单（Sell）**：卖出资产

### 3. 撮合优先级

**价格优先**

- 买单：价格越高越好
- 卖单：价格越低越好

**时间优先**

- 相同价格下，更早的订单优先成交

### 4. 订单状态

- **Pending**：待处理
- **Accepted**：已接受，进入订单簿
- **PartiallyFilled**：部分成交
- **FullyFilled**：完全成交
- **Cancelled**：已取消
- **Rejected**：已拒绝

---

## 使用示例

### 1. 使用 SDK 提交订单

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

    // 查询订单状态
    let order = client.get_order(&response.order_id).await?;
    println!("订单状态: {:?}", order.status);

    Ok(())
}
```

### 2. 使用 HTTP API 提交订单

```bash
# 生成密钥对（使用 SDK 工具）
# ...

# 签名订单请求
# ...

# 提交订单
curl -X POST http://localhost:8080/api/v1/orders \
  -H "Content-Type: application/json" \
  -d '{
    "market": "BTC-USDT",
    "side": "buy",
    "type": "limit",
    "price": 50000,
    "size": 1,
    "client_order_id": "my_order_1",
    "signature": "your_signature_here"
  }'

# 响应：
# {
#   "order_id": "uuid-here",
#   "status": "accepted"
# }
```

### 3. 查询订单

```bash
curl http://localhost:8080/api/v1/orders/{order_id}

# 响应：
# {
#   "order_id": "uuid-here",
#   "market": "BTC-USDT",
#   "side": "buy",
#   "price": 50000,
#   "size": 1,
#   "filled_size": 0,
#   "remaining_size": 1,
#   "status": "accepted",
#   ...
# }
```

### 4. 取消订单

```bash
curl -X DELETE http://localhost:8080/api/v1/orders/{order_id}

# 响应：
# {
#   "success": true
# }
```

---

## 常见问题

### Q1: 如何配置多个市场？

**A**: 在 Gateway 的配置中设置市场到撮合引擎的映射：

```rust
// 在 router.rs 中
engines.insert("BTC-USDT".to_string(), "http://localhost:50051".to_string());
engines.insert("ETH-USDT".to_string(), "http://localhost:50052".to_string());
```

### Q2: 如何支持新的区块链？

**A**: 在 Settlement 服务中添加链特定实现：

1. 在 `chains/` 目录下创建新的链模块
2. 实现 `TransactionBuilder` trait
3. 在 `submitter.rs` 中添加提交逻辑

### Q3: 如何提高性能？

**A**: 性能优化建议：

1. **增加 Gateway 工作线程**：

   ```bash
   GATEWAY_WORKERS=8 cargo run --release -p anvil-gateway
   ```

2. **使用多撮合引擎**：为不同市场分配独立的撮合引擎

3. **优化网络**：使用本地网络或低延迟网络连接

### Q4: 如何处理订单取消？

**A**: 当前实现中，订单取消功能需要：

1. Gateway 接收取消请求
2. 通过 gRPC 发送到撮合引擎
3. 撮合引擎从订单簿中移除订单

### Q5: 如何监控系统状态？

**A**: 监控建议：

1. **健康检查**：使用 `/health` 端点
2. **日志**：使用 `RUST_LOG` 环境变量控制日志级别
3. **指标**：集成 Prometheus 等监控系统（待实现）

---

## 下一步

- 📖 阅读 [架构指南](architecture-guide-zh.md) 了解详细设计
- 📖 阅读 [路由与撮合引擎](routing-and-matching-zh.md) 了解工作原理
- 🔧 查看源代码了解实现细节
- 🐛 报告问题或提出建议

---

## 获取帮助

- **文档**：查看 `docs/` 目录
- **问题**：在 GitHub Issues 中提问
- **讨论**：参与项目讨论
