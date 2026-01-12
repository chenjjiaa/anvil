# Anvil Matching Engine 性能基准测试 - 技术文档

## 概述

性能基准测试框架包含两类测试：A 类（Criterion 进程内）和 B 类（ghz 黑盒），覆盖三种测试场景，提供自动化流程和完整分析工具。

## 测试架构

### A 类测试架构

```
多Producer线程 → IngressQueue (MPSC) → MatchingLoop (单线程)
                                           ↓
                                       EventBuffer (SPSC)
                                           ↓
                                       EventWriter
                                           ↓
                                       MemoryStorage
```

**特点**：

- **无 gRPC 开销**：直接调用 `IngressQueue::try_enqueue()`
- **多生产者**：模拟并发压力，测试队列竞争
- **固定时长**：30 秒持续写入，统计吞吐量
- **三种场景**：测试不同撮合复杂度

### B 类测试架构

```
ghz并发客户端 → gRPC Server (多线程) → IngressQueue
                                          ↓
                                      MatchingEngine
```

**特点**：

- **黑盒测试**：完整的 RPC 链路
- **真实负载**：包含网络、序列化、反序列化开销
- **可调 QPS**：ghz 精确控制请求速率
- **延迟分布**：p50/p99/p999 完整指标

## 测试场景设计

### NoCross（不成交）

- **目的**：测试纯入队吞吐（无撮合开销）
- **实现**：买单价格 < 45000，卖单价格 > 55000，永不交叉
- **预期**：最高吞吐量，反映队列和入队逻辑性能

### CrossHeavy（高成交）

- **目的**：测试撮合逻辑极限
- **实现**：所有订单在 50000 价格交叉，每个都成交
- **预期**：较低吞吐量，高 events/s，反映撮合计算开销

### DeepBook（深簿部分成交）

- **目的**：测试真实市场场景
- **实现**：预填充 10k 卖单，50% taker 会部分成交
- **预期**：中等吞吐量，反映真实市场表现

## 文件清单

```
crates/matching/
├── Cargo.toml                          # 添加 Criterion 依赖和 bench 配置
├── configs/
│   └── bench.toml                      # 基准测试配置
├── src/
│   └── bin/
│       └── bench_server.rs             # 专用基准测试服务器
├── benches/
│   ├── engine_throughput.rs            # A 类测试主文件
│   └── common/
│       ├── mod.rs                      # 模块导出
│       ├── order_generator.rs          # 三种场景订单生成器
│       └── metrics.rs                  # 统计指标

scripts/
├── run_all_benchmarks.sh               # 总控脚本
├── bench_ack.sh                        # B 类 ghz 压测
├── analyze_results.py                  # 结果分析
└── monitor.sh                          # 资源监控
```

## 实施要点

### A 类测试实现

- **文件**：`crates/matching/benches/engine_throughput.rs`
- **并发级别**：1/2/4/8/16/32 个生产者线程
- **测试时长**：每个场景 30 秒
- **统计指标**：入队数、失败数、吞吐量（orders/s）
- **报告**：自动生成 Criterion HTML 报告

### B 类测试实现

- **Benchmark Server**：`crates/matching/src/bin/bench_server.rs`

  - 精简版服务器，关闭 OpenTelemetry 和详细日志
  - 队列大小：1,000,000
  - 事件缓冲：1,000,000

- **ghz 测试脚本**：`scripts/bench_ack.sh`

  - B1 场景：ACK-only（不成交）
  - B2 场景：End-to-end（成交）
  - 并发：10/50/100/200/500 连接
  - QPS：1k/5k/10k/20k/50k/100k

- **结果分析**：`scripts/analyze_results.py`
  - 解析 ghz JSON 输出
  - 生成表格化摘要
  - 计算拒绝率和过载率

### 订单生成器

- **文件**：`crates/matching/benches/common/order_generator.rs`
- **NoCross**：不成交（买单 < 45000，卖单 > 55000）
- **CrossHeavy**：高成交（所有订单在 50000 交叉）
- **DeepBook**：深簿（50% taker，50% maker）

### 统计模块

- **文件**：`crates/matching/benches/common/metrics.rs`
- **指标**：入队数、失败数、吞吐量（orders/s）

## 预期性能基线

基于设计目标和类似系统：

### A 类测试（参考值）

| 场景        | 1p   | 8p   | 32p   | 说明           |
| ----------- | ---- | ---- | ----- | -------------- |
| no_cross    | 450k | 1.2M | 1.15M | 饱和点在 8p    |
| cross_heavy | 120k | 350k | 320k  | 受撮合逻辑限制 |
| deep_book   | 200k | 500k | 480k  | 真实场景       |

### B 类测试（参考值）

| 连接数 | ACK QPS | p99 延迟 | 说明     |
| ------ | ------- | -------- | -------- |
| 100    | 50k     | <10ms    | 舒适区   |
| 200    | 80k     | <30ms    | 接近饱和 |
| 500    | 100k    | <100ms   | 饱和点   |

## 结果解读

### 关键指标

- **吞吐量（orders/s）**：每秒处理的订单数
- **延迟（p50/p99）**：50%/99% 请求的响应时间
- **拒绝率**：队列满导致的拒绝比例
- **饱和点**：增加并发不再提升吞吐的临界点

### 结果分析要点

关注以下关键指标：

1. **撮合饱和点**：orders/s 不再随生产者线程数增长的点
2. **延迟突变点**：p99 突然大幅增长的 QPS
3. **拒绝率阈值**：拒绝率超过 10% 时的 QPS
4. **资源利用率**：CPU、内存、队列占用率

根据这些指标，可以确定：

- 系统的最大安全吞吐量
- 需要优化的瓶颈环节
- 是否需要扩展队列大小

### 输出示例

**A 类测试输出**：

```
no_cross/1p             time:   [30.002 s]
[1p] 入队: 13500000 orders, 失败: 0, 吞吐: 450000 orders/s

no_cross/8p             time:   [30.001 s]
[8p] 入队: 36000000 orders, 失败: 0, 吞吐: 1200000 orders/s
```

**B 类测试输出**：

```
[ACK-only Mode Results Summary]
-------------------------------
Connections | Target QPS | Actual QPS | p50 (ms) | p99 (ms) | Error Rate
------------+------------+------------+----------+----------+-----------
         10 |    1000000 |   50859.45 |    0.089 |    0.288 |      0.00%
         10 |     100000 |   51708.17 |    0.087 |    0.274 |      0.00%
         10 |     200000 |   51774.11 |    0.089 |    0.261 |      0.00%
         10 |     500000 |   51867.29 |    0.088 |    0.269 |      0.00%
         10 |      50000 |   49782.84 |    0.085 |    0.303 |      0.00%


[End-to-end Mode Results Summary]
---------------------------------
Connections | Target QPS | Actual QPS | Accepted | p50 (ms) | p99 (ms) | Error Rate
------------+------------+------------+----------+----------+----------+-----------
         10 |    1000000 |   50333.75 |   503265 |    0.088 |    0.266 |      0.00%
         10 |     100000 |   38978.75 |   389725 |    0.106 |    0.515 |      0.00%
         10 |     200000 |   50318.85 |   503128 |    0.088 |    0.272 |      0.00%
         10 |     500000 |   50198.17 |   501917 |    0.087 |    0.271 |      0.00%
         10 |      50000 |   47984.40 |   479793 |    0.090 |    0.312 |      0.00%
```

## 性能优化方向

根据测试结果，可能的瓶颈和优化方向：

### 1. EventWriter 瓶颈

**症状**：吞吐量受限于事件写入速度

**优化方案**：

- 增大 `event_batch_size`（当前 1000）
- 减小 `event_batch_timeout_ms`（当前 50ms）
- 考虑多 EventWriter（sharding）

### 2. Orderbook 查找慢

**症状**：撮合逻辑成为瓶颈

**优化方案**：

- 当前：`BTreeMap<u64, PriceLevel>` + `Vec<Order>`
- 优化：添加 `HashMap<OrderId, OrderRef>` 索引

### 3. Journal 锁竞争严重

**症状**：多线程访问 Journal 时性能下降

**优化方案**：

- 当前：`Arc<Mutex<OrderJournal>>`
- 优化：Sharded HashMap 或 lock-free queue

### 4. gRPC 解码慢

**症状**：B 类测试延迟高，CPU 使用率低

**优化方案**：

- 启用 protobuf 零拷贝优化
- 考虑 Cap'n Proto 或 FlatBuffers

## 风险与限制

1. **机器依赖**：结果高度依赖 CPU 型号和核心数
2. **内存需求**：100w 队列需要约 2-4 GB RAM
3. **ghz 依赖**：需要单独安装 Go 和 ghz
4. **测试时间**：全量测试需 2-3 小时
5. **环境干扰**：其他进程会影响结果准确性

## 验证状态

✅ **编译通过**：

```bash
cargo check --package anvil-matching --benches --bins
```

✅ **测试通过**：

```bash
cargo test --package anvil-matching --lib
# 20/20 passed
```

✅ **脚本权限**：

```bash
ls -la scripts/*.sh
# 所有脚本均有执行权限 (rwxr-xr-x)
```

## 配置说明

基准测试使用 `crates/matching/configs/bench.toml` 配置文件：

```toml
ingress_queue_size = 1000000    # 入队队列大小
event_buffer_size = 1000000     # 事件缓冲区大小
event_batch_size = 1000         # 事件批次大小
event_batch_timeout_ms = 50     # 批次超时
verbose_logging = false         # 关闭详细日志以减少开销
```

**调整建议**：

- 内存不足时：减小 `ingress_queue_size` 和 `event_buffer_size`
- EventWriter 瓶颈时：增大 `event_batch_size` 或减小 `event_batch_timeout_ms`

## 使用流程

### 快速测试（单个场景）

```bash
# 只测试 no_cross/8p
cargo bench --package anvil-matching --bench engine_throughput -- "no_cross/8p"
```

### 全量测试（2-3 小时）

```bash
./scripts/run_all_benchmarks.sh
```

### 查看结果

- **Criterion HTML**：`target/criterion/report/index.html`
- **ghz JSON**：`benchmark_results/*.json`
- **分析摘要**：运行 `python3 scripts/analyze_results.py`

## 监控资源使用

在运行测试时，可以同步监控 CPU 和内存：

```bash
# 获取 bench-server 的 PID
ps aux | grep bench-server | grep -v grep

# 启动监控
./scripts/monitor.sh <PID>
```

监控数据保存在 `benchmark_results/monitor.log`，格式为 CSV。

## 注意事项

1. **机器配置影响巨大**：记录 CPU 型号、核心数、内存大小
2. **关闭其他负载**：测试前关闭其他应用程序
3. **测试时间较长**：全量测试需要 2-3 小时
4. **内存需求**：100w 队列大约需要 2-4 GB 内存
5. **结果可重现性**：多次运行取平均值
6. **CPU 使用率**：撮合引擎是单线程设计，单核应接近 100%，其他核心处理 gRPC 和事件写入

## 总结

性能基准测试框架已完整实施，包括：

- **2 类测试**：A 类（Criterion 进程内）+ B 类（ghz 黑盒）
- **3 种场景**：NoCross / CrossHeavy / DeepBook
- **自动化流程**：一键运行 + 自动分析
- **完整工具**：监控、分析、报告生成

所有代码已通过编译和测试验证，可以立即使用。建议首次运行后建立性能基线，作为后续优化的对比参考。
