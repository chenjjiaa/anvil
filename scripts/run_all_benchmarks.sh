#!/bin/bash

set -e

echo "====================================="
echo "Matching Engine Performance Benchmark"
echo "====================================="
echo ""

# 1. Build release version
echo "[1/5] Building Release version..."
cargo build --release --package anvil-matching

# 2. Run Class A tests (Criterion)
echo ""
echo "[2/5] Running Class A tests (matching throughput)..."
cargo bench --package anvil-matching --bench engine_throughput

# 3. Start benchmark server
echo ""
echo "[3/5] Starting Benchmark Server..."
cargo run --release --bin bench-server &
SERVER_PID=$!
sleep 5

# 4. Run Class B tests (ghz)
echo ""
echo "[4/5] Running Class B tests (gRPC load test)..."
bash scripts/bench_ack.sh

# 5. Stop server
echo ""
echo "[5/5] Stopping Server..."
kill $SERVER_PID

# 6. Analyze results
echo ""
echo "Analyzing results..."
python3 scripts/analyze_results.py

echo ""
echo "======================================"
echo "Benchmark completed!"
echo "- Criterion results: target/criterion/"
echo "- ghz results: benchmark_results/"
echo "======================================"

