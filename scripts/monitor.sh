#!/bin/bash

PID=$1
OUTPUT="benchmark_results/monitor.log"

echo "Monitoring PID: $PID" > "$OUTPUT"
echo "Timestamp,CPU%,Memory(MB)" >> "$OUTPUT"

while kill -0 $PID 2>/dev/null; do
    timestamp=$(date +%s)
    cpu=$(ps -p $PID -o %cpu | tail -1 | xargs)
    mem=$(ps -p $PID -o rss | tail -1 | xargs)
    mem_mb=$((mem / 1024))
    
    echo "$timestamp,$cpu,$mem_mb" >> "$OUTPUT"
    sleep 1
done
