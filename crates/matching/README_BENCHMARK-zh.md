# 性能基准测试

本目录包含撮合引擎的完整性能基准测试套件。

## 快速开始

### 前置条件

1. Rust 工具链
2. ghz（用于 B 类测试）：
   ```bash
   go install github.com/bojand/ghz/cmd/ghz@latest
   ```
3. Python 3（用于结果分析）

### 一键运行全部测试（推荐）

```bash
./scripts/run_all_benchmarks.sh
```

这会自动执行：

1. 构建 Release 版本
2. 运行 A 类测试（Criterion）
3. 启动 benchmark server
4. 运行 B 类测试（ghz）
5. 停止 server
6. 分析并输出结果

### 分别运行测试

#### A 类测试：Criterion 基准测试

```bash
# 运行所有场景
cargo bench --package anvil-matching --bench engine_throughput

# 只测试特定场景
cargo bench --package anvil-matching --bench engine_throughput -- "no_cross/8p"

# 查看 HTML 报告
open target/criterion/report/index.html
```

#### B 类测试：ghz gRPC 压测

1. **启动 benchmark server**（终端 1）：

   ```bash
   cargo run --release --bin bench-server
   ```

   等待看到：`Server ready for benchmarking`

2. **运行压测**（终端 2）：

   ```bash
   ./scripts/bench_ack.sh
   ```

3. **分析结果**：
   ```bash
   python3 scripts/analyze_results.py
   ```

## 测试类型

### A 类：Criterion 进程内基准测试

- **工具**：`criterion.rs`
- **测试对象**：撮合核心吞吐能力（无 gRPC 开销）
- **并发级别**：1/2/4/8/16/32 个生产者线程
- **测试时长**：每个场景 30 秒

### B 类：ghz gRPC 黑盒压测

- **工具**：`ghz`
- **测试对象**：gRPC ACK 能力和端到端性能（完整 RPC 链路）
- **并发级别**：10/50/100/200/500 个连接
- **QPS 目标**：1k/5k/10k/20k/50k/100k
- **测试时长**：每个场景 30 秒

## 测试场景

### NoCross（不成交）

- **目的**：测试纯入队吞吐（无撮合开销）
- **特点**：买单价格 < 45000，卖单价格 > 55000，永不交叉
- **预期**：最高吞吐量

### CrossHeavy（高成交）

- **目的**：测试撮合逻辑极限
- **特点**：所有订单在 50000 价格交叉，每个都成交
- **预期**：较低吞吐量，高 events/s

### DeepBook（深簿部分成交）

- **目的**：测试真实市场场景
- **特点**：预填充 10k 卖单，50% taker 会部分成交
- **预期**：中等吞吐量，反映真实表现

## 预期结果（参考值）

### A 类测试

| 场景        | 1p   | 8p   | 32p   | 说明           |
| ----------- | ---- | ---- | ----- | -------------- |
| no_cross    | 450k | 1.2M | 1.15M | 饱和点在 8p    |
| cross_heavy | 120k | 350k | 320k  | 受撮合逻辑限制 |
| deep_book   | 200k | 500k | 480k  | 真实场景       |

### B 类测试

| 连接数 | ACK QPS | p99 延迟 | 说明     |
| ------ | ------- | -------- | -------- |
| 100    | 50k     | <10ms    | 舒适区   |
| 200    | 80k     | <30ms    | 接近饱和 |
| 500    | 100k    | <100ms   | 饱和点   |

## 配置

基准测试使用 `crates/matching/configs/bench.toml`：

```toml
ingress_queue_size = 1000000    # 入队队列大小
event_buffer_size = 1000000     # 事件缓冲区大小
event_batch_size = 1000         # 事件批次大小
event_batch_timeout_ms = 50     # 批次超时
verbose_logging = false         # 关闭详细日志以减少开销
```

## 监控资源使用

启动 bench-server 后，在另一个终端：

```bash
# 找到 PID
ps aux | grep bench-server | grep -v grep

# 启动监控（替换 <PID>）
./scripts/monitor.sh <PID>

# 查看监控日志
tail -f benchmark_results/monitor.log
```

## 结果查看

- **Criterion HTML**：`target/criterion/report/index.html`
- **ghz JSON**：`benchmark_results/*.json`
- **分析摘要**：运行 `python3 scripts/analyze_results.py`

## 故障排查

### ghz 报错 "rpc error: code = Unavailable"

**解决**：确保 bench-server 正在运行：

```bash
lsof -i :50051
```

### Criterion 报错 "Permission denied"

**解决**：确保脚本有执行权限：

```bash
chmod +x scripts/*.sh
```

### 内存不足

**解决**：减小队列大小，编辑 `configs/bench.toml`：

```toml
ingress_queue_size = 500000   # 从 1000000 减少
event_buffer_size = 500000
```

### CPU 使用率不高

这是正常的，因为撮合引擎是**单线程**设计。观察：

- 单核 CPU 应该接近 100%
- 其他核心处理 gRPC 请求和事件写入

## 自定义测试

### 修改测试参数

编辑 `scripts/bench_ack.sh`：

```bash
# 修改并发连接数
CONNECTIONS=(10 50 100 200 500)

# 修改目标 QPS
QPS_RATES=(1000 5000 10000 20000 50000 100000)
```

### 自定义 ghz 测试

```bash
# 测试不成交场景：100 连接，10000 QPS，持续 30 秒
ghz --insecure \
    --proto="crates/matching/proto/matching.proto" \
    --import-paths="crates/matching/proto" \
    --call=anvil.matching.MatchingService/SubmitOrder \
    --connections=100 \
    --concurrency=100 \
    --rps=10000 \
    --duration=30s \
    --data='{"order_id":"test-{{.RequestNumber}}","market":"BTC-USDT","side":"BUY","price":44000,"size":1,"timestamp":1704067200,"public_key":"test"}' \
    localhost:50051
```

## 注意事项

1. **机器配置影响巨大**：记录 CPU 型号、核心数、内存大小
2. **关闭其他负载**：测试前关闭其他应用程序
3. **测试时间较长**：全量测试需要 2-3 小时
4. **内存需求**：100w 队列大约需要 2-4 GB 内存
5. **结果可重现性**：多次运行取平均值

## 详细文档

查看 [BENCHMARK.md](BENCHMARK.md) 了解：

- 技术架构细节
- 测试场景设计原理
- 性能优化方向
- 实施要点和文件清单
