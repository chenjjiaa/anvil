#!/bin/bash

set -e

echo "======================================"
echo "Matching Engine Performance Benchmark"
echo "======================================"
echo ""

# Run Class B tests (ghz)
echo "[1/3] Starting Benchmark Server..."
cargo run --release --bin bench-server &
SERVER_PID=$!
sleep 5

echo "[2/3] Running Class B tests (gRPC load test)..."
bash scripts/bench_ack.sh

# Stop server
echo ""
echo "[3/3] Stopping Server..."
kill $SERVER_PID

# Analyze results
echo ""
echo "Analyzing results..."
python3 scripts/analyze_results.py

echo ""
echo "======================================"
echo "Benchmark completed!"
echo "- Criterion results: target/criterion/"
echo "- ghz results: benchmark_results/"
echo "======================================"
