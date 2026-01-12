#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

def analyze_ghz_result(filepath):
    """Parse JSON result from ghz output."""
    with open(filepath) as f:
        data = json.load(f)
    
    total = data.get('count', 0)
    avg_ns = data.get('average', 0)  # nanoseconds
    fastest_ns = data.get('fastest', 0)
    slowest_ns = data.get('slowest', 0)
    
    # Extract percentiles from latencyDistribution (nanoseconds)
    latency_dist = data.get('latencyDistribution')
    p50_ns = 0
    p99_ns = 0
    
    if latency_dist:
        if isinstance(latency_dist, dict):
            p50_ns = latency_dist.get('p50', 0)
            p99_ns = latency_dist.get('p99', 0)
        elif isinstance(latency_dist, list):
            # If list format, iterate to find p50 and p99
            for item in latency_dist:
                if isinstance(item, dict):
                    # ghz uses "percentage" key, e.g., {"percentage": 50, "latency": 73208}
                    percentage = item.get('percentage') or item.get('percentile') or item.get('p')
                    latency = item.get('latency') or item.get('value')
                    if percentage == 50 or percentage == '50':
                        p50_ns = latency or 0
                    elif percentage == 99 or percentage == '99':
                        p99_ns = latency or 0
                elif isinstance(item, list) and len(item) >= 2:
                    # May be in [50, 1000] format
                    if item[0] == 50:
                        p50_ns = item[1]
                    elif item[0] == 99:
                        p99_ns = item[1]
    
    # Convert to milliseconds
    avg_ms = avg_ns / 1_000_000
    p50_ms = p50_ns / 1_000_000
    p99_ms = p99_ns / 1_000_000
    
    # Get actual QPS from rps field
    actual_rps = data.get('rps', 0)
    
    # Count status codes from statusCodeDistribution (primary source)
    status_dist = data.get('statusCodeDistribution')
    accepted = 0
    rejected = 0
    
    if status_dist:
        if isinstance(status_dist, dict):
            # Iterate through all status codes
            for status, count in status_dist.items():
                count = int(count) if isinstance(count, (int, float)) else 0
                # gRPC status code: 0, '0', 'OK' all indicate success
                status_str = str(status).upper()
                if status == 0 or status == '0' or status_str == 'OK':
                    accepted += count
                else:
                    # Other status codes are treated as errors
                    rejected += count
        elif isinstance(status_dist, list):
            # If list format, attempt to parse
            rejected = len(status_dist)
    
    # If statusCodeDistribution has no data or counts to 0, infer from errorDistribution
    if accepted == 0 and rejected == 0 and total > 0:
        error_dist = data.get('errorDistribution')
        if error_dist:
            if isinstance(error_dist, dict):
                rejected = sum(int(v) for v in error_dist.values() if isinstance(v, (int, float)))
            elif isinstance(error_dist, list):
                rejected = len(error_dist)
        accepted = total - rejected
    
    # Ensure accepted + rejected = total (data consistency check)
    if accepted + rejected != total and total > 0:
        # If counts don't match, use total as baseline and recalculate accepted
        accepted = total - rejected
    
    return {
        'total': total,
        'avg_ms': avg_ms,
        'p50_ms': p50_ms,
        'p99_ms': p99_ms,
        'accepted': accepted,
        'rejected': rejected,
        'qps': actual_rps,
    }

def display_width(s):
    """Calculate display width of string in terminal (CJK characters occupy 2 positions)."""
    width = 0
    for char in str(s):
        # CJK character range (Unified CJK Ideographs, Chinese punctuation, etc.)
        if '\u4e00' <= char <= '\u9fff' or '\u3000' <= char <= '\u303f' or '\uff00' <= char <= '\uffef':
            width += 2
        else:
            width += 1
    return width

def print_table(headers, rows):
    """Print table with fixed width, ensuring column alignment."""
    # Calculate maximum display width for each column (including header and data rows, take the widest)
    col_widths = []
    for i, header in enumerate(headers):
        # Initialize max width as header display width
        max_width = display_width(header)
        # Iterate through all data rows to find the maximum display width for this column
        for row in rows:
            if i < len(row):
                cell_value = str(row[i])
                max_width = max(max_width, display_width(cell_value))
        # Store column width (display width)
        col_widths.append(max_width)
    
    def format_cell(value, width):
        """Format cell: right-align, considering CJK character display width."""
        val_str = str(value)
        val_width = display_width(val_str)
        padding = width - val_width
        return ' ' * padding + val_str
    
    # Print header (right-aligned, consistent with data)
    header_line = ' | '.join(format_cell(headers[i], col_widths[i]) for i in range(len(headers)))
    print(header_line)
    
    # Print separator line
    sep_line = '-+-'.join('-' * col_widths[i] for i in range(len(headers)))
    print(sep_line)
    
    # Print data rows (right-aligned)
    for row in rows:
        data_line = ' | '.join(format_cell(row[i] if i < len(row) else '', col_widths[i]) for i in range(len(headers)))
        print(data_line)

def main():
    results_dir = Path('benchmark_results')
    if not results_dir.exists():
        print("Error: Results directory does not exist")
        return
    
    print("")
    print("[ACK-only Mode Results Summary]")
    print("-------------------------------")
    
    # Collect data
    ack_data = []
    for file in sorted(results_dir.glob('ack_*.json')):
        parts = file.stem.split('_')
        conn = parts[1].replace('c', '')
        qps = parts[2].replace('qps', '')
        
        try:
            result = analyze_ghz_result(file)
            error_rate = result['rejected'] / result['total'] * 100 if result['total'] > 0 else 0
            
            ack_data.append([
                conn,
                qps,
                f"{result['qps']:.2f}",
                f"{result['p50_ms']:.3f}",
                f"{result['p99_ms']:.3f}",
                f"{error_rate:.2f}%"
            ])
        except Exception as e:
            ack_data.append([conn, qps, f"Parse error: {e}", "", "", ""])
            import traceback
            traceback.print_exc()
    
    # Print table
    headers = ['Connections', 'Target QPS', 'Actual QPS', 'p50 (ms)', 'p99 (ms)', 'Error Rate']
    print_table(headers, ack_data)
    
    print("")
    print("")
    print("[End-to-end Mode Results Summary]")
    print("---------------------------------")
    
    # Collect data
    e2e_data = []
    for file in sorted(results_dir.glob('e2e_*.json')):
        parts = file.stem.split('_')
        conn = parts[1].replace('c', '')
        qps = parts[2].replace('qps', '')
        
        try:
            result = analyze_ghz_result(file)
            error_rate = result['rejected'] / result['total'] * 100 if result['total'] > 0 else 0
            
            e2e_data.append([
                conn,
                qps,
                f"{result['qps']:.2f}",
                f"{result['accepted']}",
                f"{result['p50_ms']:.3f}",
                f"{result['p99_ms']:.3f}",
                f"{error_rate:.2f}%"
            ])
        except Exception as e:
            e2e_data.append([conn, qps, f"Parse error: {e}", "", "", "", ""])
            import traceback
            traceback.print_exc()
    
    # Print table
    headers = ['Connections', 'Target QPS', 'Actual QPS', 'Accepted', 'p50 (ms)', 'p99 (ms)', 'Error Rate']
    print_table(headers, e2e_data)

if __name__ == '__main__':
    main()
    print("")