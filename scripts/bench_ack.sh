#!/bin/bash

# ghz gRPC load test script

PROTO_PATH="crates/matching/proto/matching.proto"
IMPORT_PATH="crates/matching/proto"
SERVER_ADDR="localhost:50051"
RESULTS_DIR="benchmark_results"

mkdir -p "$RESULTS_DIR"

echo "===================================="
echo "B1: ACK-only Mode (No Cross Scenario)"
echo "===================================="

# Different concurrency and QPS combinations
# CONNECTIONS=(10 50 100 200 500)
# QPS_RATES=(1000 5000 10000 20000 50000 100000)
CONNECTIONS=(10)
QPS_RATES=(50000 100000 200000 500000 1000000)

for conn in "${CONNECTIONS[@]}"; do
    for qps in "${QPS_RATES[@]}"; do
        echo "Test: ${conn} connections, ${qps} QPS"
        
        ghz --insecure \
            --proto="$PROTO_PATH" \
            --import-paths="$IMPORT_PATH" \
            --call=anvil.matching.MatchingService/SubmitOrder \
            --connections="$conn" \
            --concurrency="$conn" \
            --rps="$qps" \
            --duration=10s \
            --data='{"order_id":"bench-{{.RequestNumber}}","market":"BTC-USDT","side":"BUY","price":44000,"size":1,"timestamp":1704067200,"public_key":"bench"}' \
            --format=json \
            --output="$RESULTS_DIR/ack_${conn}c_${qps}qps.json" \
            "$SERVER_ADDR"
        
        sleep 2
    done
done

echo ""
echo "===================================="
echo "B2: End-to-end Mode (Cross Scenario)"
echo "===================================="

# Cross scenario: price in cross range
for conn in "${CONNECTIONS[@]}"; do
    for qps in "${QPS_RATES[@]}"; do
        echo "Test: ${conn} connections, ${qps} QPS (cross)"
        
        ghz --insecure \
            --proto="$PROTO_PATH" \
            --import-paths="$IMPORT_PATH" \
            --call=anvil.matching.MatchingService/SubmitOrder \
            --connections="$conn" \
            --concurrency="$conn" \
            --rps="$qps" \
            --duration=10s \
            --data='{"order_id":"bench-{{.RequestNumber}}","market":"BTC-USDT","side":"{{if eq (mod .RequestNumber 2) 0}}BUY{{else}}SELL{{end}}","price":50000,"size":10,"timestamp":1704067200,"public_key":"bench"}' \
            --format=json \
            --output="$RESULTS_DIR/e2e_${conn}c_${qps}qps.json" \
            "$SERVER_ADDR"
        
        sleep 2
    done
done

echo ""
echo "Load test completed! Results saved in: $RESULTS_DIR"
