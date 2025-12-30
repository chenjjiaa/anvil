# 路由与撮合引擎工作原理（中文版）

## 目录

1. [路由模块详解](#路由模块详解)
2. [撮合引擎详解](#撮合引擎详解)
3. [订单簿数据结构](#订单簿数据结构)
4. [撮合算法详解](#撮合算法详解)
5. [性能优化策略](#性能优化策略)

---

## 路由模块详解

### 概述

路由模块（`Router`）是 Gateway 服务的核心组件，负责将客户端提交的订单路由到对应的撮合引擎。它维护了市场标识符到撮合引擎端点的映射关系，并管理 gRPC 客户端连接。

### 架构设计

```rust
pub struct Router {
    /// 市场 -> 撮合引擎端点映射
    matching_engines: HashMap<String, String>,
    /// 市场 -> gRPC 客户端映射（带互斥锁用于异步访问）
    clients: Arc<Mutex<HashMap<String, MatchingGrpcClient>>>,
}
```

### 核心功能

#### 1. 市场到撮合引擎映射

路由模块维护一个 `HashMap`，将市场标识符（如 "BTC-USDT"）映射到对应的撮合引擎 gRPC 端点（如 "http://localhost:50051"）。

```rust
// 初始化示例
let mut engines = HashMap::new();
engines.insert("BTC-USDT".to_string(), "http://localhost:50051".to_string());
engines.insert("ETH-USDT".to_string(), "http://localhost:50052".to_string());
```

**设计考虑**：

- 支持多市场：每个市场可以有独立的撮合引擎
- 可配置：映射关系可以通过配置文件或环境变量设置
- 动态扩展：支持运行时添加新的市场映射

#### 2. gRPC 客户端管理

路由模块使用延迟初始化和连接复用的策略来管理 gRPC 客户端：

```rust
async fn get_client(&self, market: &str) -> Result<MatchingGrpcClient, RouterError> {
    // 1. 查找市场对应的端点
    let endpoint = self.matching_engines.get(market)?;

    // 2. 检查是否已有客户端连接
    let mut clients = self.clients.lock().await;

    if let Some(client) = clients.get(market) {
        // 复用现有连接（tonic 客户端可以低成本克隆）
        Ok(client.clone())
    } else {
        // 创建新连接
        let client = MatchingGrpcClient::new(endpoint).await?;
        let client_clone = client.clone();
        clients.insert(market.to_string(), client);
        Ok(client_clone)
    }
}
```

**设计优势**：

- **连接复用**：避免频繁创建和销毁连接
- **延迟初始化**：只在需要时创建连接
- **线程安全**：使用 `Mutex` 保护客户端映射的并发访问
- **低成本克隆**：tonic 客户端支持低成本克隆，共享底层连接

#### 3. 订单格式转换

路由模块将 Gateway 的 `PlaceOrderRequest` 转换为撮合引擎的 `Order` 格式：

```rust
pub async fn route_order(
    &self,
    request: PlaceOrderRequest,
    public_key: String,
) -> Result<MatchingOrder, RouterError> {
    // 1. 提取价格（限价单必须提供价格）
    let price = request.price
        .ok_or_else(|| RouterError::RoutingError("Limit orders require a price".to_string()))?;

    // 2. 生成订单 ID
    let order_id = uuid::Uuid::new_v4().to_string();

    // 3. 构建撮合引擎订单
    let order = MatchingOrder {
        order_id,
        market: request.market.clone(),
        side: request.side,
        price,
        size: request.size,
        remaining_size: request.size,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        public_key,
    };

    // 4. 获取 gRPC 客户端并提交订单
    let mut client = self.get_client(&request.market).await?;
    client.submit_order(order.clone()).await?;

    Ok(order)
}
```

**转换要点**：

- **订单 ID 生成**：使用 UUID v4 生成唯一订单 ID
- **时间戳设置**：使用当前 Unix 时间戳
- **剩余数量初始化**：初始时 `remaining_size = size`
- **用户 ID 传递**：从认证信息中提取的用户 ID

### 路由流程

```
客户端订单请求
    │
    ├─> 提取市场标识符
    │
    ├─> 查找市场映射
    │     └─> 如果不存在：返回错误
    │
    ├─> 获取或创建 gRPC 客户端
    │     ├─> 检查缓存
    │     ├─> 如果存在：复用连接
    │     └─> 如果不存在：创建新连接并缓存
    │
    ├─> 转换订单格式
    │     ├─> PlaceOrderRequest → Order
    │     ├─> 生成订单 ID
    │     └─> 设置时间戳
    │
    └─> 通过 gRPC 提交订单
          └─> 返回订单信息
```

---

## 撮合引擎详解

### 概述

撮合引擎（`Matcher`）是系统的核心，负责维护订单簿并执行撮合逻辑。它实现了确定性的价格-时间优先级匹配算法。

### 架构设计

```rust
pub struct Matcher {
    /// 市场 -> 订单簿映射（并发访问）
    order_books: DashMap<String, OrderBook>,
}
```

**设计特点**：

- **多市场支持**：每个市场有独立的订单簿
- **并发访问**：使用 `DashMap` 实现无锁并发访问
- **延迟创建**：订单簿按需创建

### 订单簿数据结构

#### OrderBook 结构

```rust
pub struct OrderBook {
    market: String,
    /// 买单簿：价格 -> 价格层级（并发映射）
    bids: Arc<DashMap<u64, PriceLevel>>,
    /// 卖单簿：价格 -> 价格层级（并发映射）
    asks: Arc<DashMap<u64, PriceLevel>>,
}
```

#### PriceLevel 结构

```rust
pub struct PriceLevel {
    price: u64,              // 价格
    orders: Vec<Order>,      // 订单队列（FIFO）
    total_size: u64,         // 该价格层级的总数量
}
```

**数据结构说明**：

- **DashMap**：无锁并发哈希表，支持高并发读写
- **Vec<Order>**：FIFO 队列，保证时间优先级
- **Arc**：共享所有权，支持并发访问

#### 订单簿示例

假设有以下订单：

```
买单簿（Bids）：
  50000: [buy_1(1, t=1000), buy_2(2, t=1001)]  // 价格 50000，订单 buy_1 数量 1 时间 1000
  49900: [buy_3(1, t=1002)]                    // 价格 49900，订单 buy_3 数量 1 时间 1002

卖单簿（Asks）：
  50100: [sell_1(1, t=1003)]                    // 价格 50100，订单 sell_1 数量 1 时间 1003
  50200: [sell_2(2, t=1004)]                    // 价格 50200，订单 sell_2 数量 2 时间 1004
```

**最优价格**：

- **最优买价（Best Bid）**：50000（最高买价）
- **最优卖价（Best Ask）**：50100（最低卖价）

### 撮合算法详解

#### 价格-时间优先级规则

1. **价格优先**：更好的价格优先成交

   - 买单：价格越高越好
   - 卖单：价格越低越好

2. **时间优先**：相同价格下，更早的订单优先成交
   - FIFO（先进先出）队列

#### 买单撮合流程

```rust
fn match_buy_order(orderbook: &OrderBook, order: &Order, remaining_size: u64) -> Option<Trade> {
    // 1. 获取最优卖价
    let best_ask = orderbook.best_ask()?;

    // 2. 检查价格是否可成交
    // 买单价格必须 >= 最优卖价
    if order.price < best_ask {
        return None; // 价格不可成交
    }

    // 3. 获取最优卖价层级
    let mut ask_level = orderbook.best_ask_level()?;

    // 4. 获取该价格层级的第一个订单（FIFO）
    let maker_order = ask_level.get_first_order()?.clone();

    // 5. 计算成交价格和数量
    let match_price = best_ask;  // 成交价格 = 订单簿价格（maker 价格）
    let match_size = remaining_size.min(maker_order.remaining_size);

    // 6. 更新或移除 maker 订单
    if maker_order.remaining_size == match_size {
        // 完全成交，移除订单
        ask_level.remove_first_order();
        if ask_level.is_empty() {
            // 如果价格层级为空，从订单簿移除
            orderbook.remove_order(Side::Sell, &maker_order.order_id);
        }
    } else {
        // 部分成交，更新剩余数量
        ask_level.update_order_size(
            &maker_order.order_id,
            maker_order.remaining_size - match_size,
        );
    }

    // 7. 创建成交记录
    Some(Trade {
        trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
        market: order.market.clone(),
        price: match_price,
        size: match_size,
        side: Side::Buy,
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        maker_order_id: maker_order.order_id.clone(),
        taker_order_id: order.order_id.clone(),
    })
}
```

#### 卖单撮合流程

卖单撮合流程与买单类似，但方向相反：

```rust
fn match_sell_order(orderbook: &OrderBook, order: &Order, remaining_size: u64) -> Option<Trade> {
    // 1. 获取最优买价
    let best_bid = orderbook.best_bid()?;

    // 2. 检查价格是否可成交
    // 卖单价格必须 <= 最优买价
    if order.price > best_bid {
        return None;
    }

    // 3. 获取最优买价层级
    let mut bid_level = orderbook.best_bid_level()?;

    // 4. 获取该价格层级的第一个订单（FIFO）
    let maker_order = bid_level.get_first_order()?.clone();

    // 5. 计算成交价格和数量
    let match_price = best_bid;
    let match_size = remaining_size.min(maker_order.remaining_size);

    // 6. 更新或移除 maker 订单
    // ...（与买单类似）

    // 7. 创建成交记录
    Some(Trade { ... })
}
```

#### 完整撮合流程

```rust
pub fn match_order(&self, mut order: Order) -> Result<MatchResult, MatchingError> {
    // 1. 获取或创建订单簿
    let orderbook = self.order_books
        .entry(order.market.clone())
        .or_insert_with(|| OrderBook::new(order.market.clone()))
        .clone();

    let mut trades = Vec::new();
    let mut remaining_size = order.remaining_size;

    // 2. 循环撮合直到无法继续
    while remaining_size > 0 {
        let matched = match order.side {
            Side::Buy => Self::match_buy_order(&orderbook, &order, remaining_size),
            Side::Sell => Self::match_sell_order(&orderbook, &order, remaining_size),
        };

        match matched {
            Some(trade) => {
                // 有成交，继续撮合
                trades.push(trade.clone());
                remaining_size -= trade.size;
            }
            None => {
                // 无法继续撮合，退出循环
                break;
            }
        }
    }

    // 3. 更新订单剩余数量
    order.remaining_size = remaining_size;

    // 4. 判断订单状态
    let fully_filled = remaining_size == 0;
    let partially_filled = !trades.is_empty() && remaining_size > 0;

    // 5. 如果订单未完全成交，加入订单簿
    if !fully_filled {
        orderbook.add_order(order.clone());
    }

    // 6. 返回撮合结果
    Ok(MatchResult {
        order,
        trades,
        fully_filled,
        partially_filled,
    })
}
```

### 撮合示例

#### 示例 1：完全成交

**初始状态**：

- 卖单簿：50000: [sell_1(1)]

**新订单**：

- 买单：价格 50000，数量 1

**撮合过程**：

1. 查找最优卖价：50000
2. 检查价格：50000 >= 50000 ✓
3. 获取 sell_1：数量 1
4. 计算成交：数量 = min(1, 1) = 1
5. 移除 sell_1（完全成交）
6. 买单完全成交

**结果**：

- 成交：Trade { price: 50000, size: 1, maker: sell_1, taker: buy_1 }
- 订单状态：fully_filled = true

#### 示例 2：部分成交

**初始状态**：

- 卖单簿：50000: [sell_1(1)]

**新订单**：

- 买单：价格 50000，数量 3

**撮合过程**：

1. 查找最优卖价：50000
2. 检查价格：50000 >= 50000 ✓
3. 获取 sell_1：数量 1
4. 计算成交：数量 = min(3, 1) = 1
5. 移除 sell_1（完全成交）
6. 买单剩余 2，继续撮合
7. 查找最优卖价：无（订单簿为空）
8. 无法继续撮合

**结果**：

- 成交：Trade { price: 50000, size: 1, maker: sell_1, taker: buy_1 }
- 订单状态：partially_filled = true
- 剩余订单：买单（数量 2）加入订单簿

#### 示例 3：价格-时间优先级

**初始状态**：

- 卖单簿：
  - 50000: [sell_1(1, t=1000), sell_2(1, t=1001)]
  - 50010: [sell_3(1, t=1002)]

**新订单**：

- 买单：价格 50010，数量 2

**撮合过程**：

1. 查找最优卖价：50000（最低卖价）
2. 检查价格：50010 >= 50000 ✓
3. 获取 sell_1（时间最早）：数量 1
4. 成交：Trade { price: 50000, size: 1, maker: sell_1 }
5. 剩余数量：1，继续撮合
6. 查找最优卖价：50000
7. 获取 sell_2：数量 1
8. 成交：Trade { price: 50000, size: 1, maker: sell_2 }
9. 剩余数量：0，完全成交

**结果**：

- 成交列表：
  - Trade { price: 50000, size: 1, maker: sell_1, taker: buy_1 }
  - Trade { price: 50000, size: 1, maker: sell_2, taker: buy_1 }
- 订单状态：fully_filled = true
- **注意**：虽然买单价格是 50010，但成交价格是 50000（maker 价格），买单获得了价格改善

---

## 性能优化策略

### 1. 无锁并发

**使用 DashMap**：

- `DashMap` 是线程安全的并发哈希表
- 使用无锁算法，避免互斥锁开销
- 支持高并发读写操作

**性能影响**：

- 减少锁竞争
- 提高并发吞吐量
- 降低延迟

### 2. 延迟初始化

**订单簿延迟创建**：

```rust
let orderbook = self.order_books
    .entry(order.market.clone())
    .or_insert_with(|| OrderBook::new(order.market.clone()))
    .clone();
```

**优势**：

- 节省内存：只为有交易的市场创建订单簿
- 快速启动：不需要预先创建所有市场的订单簿

### 3. 连接复用

**gRPC 客户端复用**：

- 每个市场维护一个 gRPC 客户端连接
- 避免频繁创建和销毁连接
- 减少连接建立开销

### 4. 批量处理

**成交结果批量提交**：

- 将多个成交打包到一个交易中
- 减少网络往返次数
- 降低链上交易费用

### 5. 内存优化

**及时释放**：

- 已完全成交的订单立即移除
- 空价格层级及时清理
- 避免内存泄漏

**共享所有权**：

- 使用 `Arc` 共享不可变数据
- 减少内存复制
- 提高缓存命中率

---

## 总结

路由模块和撮合引擎是 Anvil 系统的核心组件：

1. **路由模块**：

   - 负责订单的路由和格式转换
   - 管理 gRPC 客户端连接
   - 支持多市场、多撮合引擎架构

2. **撮合引擎**：

   - 维护订单簿数据结构
   - 实现价格-时间优先级匹配算法
   - 支持高并发、低延迟撮合

3. **性能优化**：
   - 无锁并发访问
   - 连接复用
   - 延迟初始化
   - 内存优化

通过这些设计，Anvil 实现了高性能、可扩展的撮合系统。
