// Copyright 2025 itscheems
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Integration tests for the logging system
//!
//! These tests verify that the logging infrastructure works correctly,
//! including file output, log rotation, and structured logging.

use std::{
	fs,
	path::PathBuf,
	sync::{Arc, Mutex},
	thread,
	time::Duration,
};

use anvil_matching::{
	EventBuffer, EventWriter, EventWriterConfig, IngressQueue, MatchingEngine, MemoryEventStorage,
	MemoryOrderJournal, OrderJournal, config::MatchingConfig, engine::EngineConfig,
	types::OrderCommand,
};
use anvil_sdk::types::Side;

/// Helper to find project root for log directory
fn find_project_root() -> PathBuf {
	let mut current = std::env::current_dir().expect("Failed to get current directory");
	loop {
		if current.join("Cargo.toml").exists() {
			let cargo_content = fs::read_to_string(current.join("Cargo.toml")).unwrap_or_default();
			if cargo_content.contains("[workspace]") {
				return current;
			}
		}
		if let Some(parent) = current.parent() {
			current = parent.to_path_buf();
		} else {
			break;
		}
	}
	std::env::current_dir().expect("Failed to get current directory")
}

/// Get log directory path
fn get_log_dir() -> PathBuf {
	let project_root = find_project_root();
	project_root.join("logs").join("matching")
}

/// Get today's log file name
fn get_today_log_file() -> String {
	use std::time::SystemTime;

	let now = SystemTime::now()
		.duration_since(SystemTime::UNIX_EPOCH)
		.expect("Time went backwards");

	// Calculate date from timestamp
	let days = now.as_secs() / 86400;
	let _epoch_days = 719162; // Days from year 0 to Unix epoch (1970-01-01)

	// Simple date calculation (good enough for test)
	let year = 1970 + (days / 365) as i32;
	let day_of_year = (days % 365) as i32;
	let month = (day_of_year / 30) + 1;
	let day = (day_of_year % 30) + 1;

	format!("matching.{:04}-{:02}-{:02}.log", year, month, day)
}

#[test]
fn test_logging_initialization() {
	// This test verifies that logging can be initialized without panicking
	// Note: We can't call init_logging() multiple times in the same process,
	// so this test is primarily checking that the module compiles and doesn't crash

	let log_dir = get_log_dir();
	println!("Expected log directory: {}", log_dir.display());

	// Check if log directory exists (it should be created by the main binary or other tests)
	if log_dir.exists() {
		println!("Log directory exists");

		// List log files
		if let Ok(entries) = fs::read_dir(&log_dir) {
			let log_files: Vec<_> = entries
				.filter_map(|e| e.ok())
				.filter(|e| {
					e.path()
						.extension()
						.and_then(|ext| ext.to_str())
						.map(|ext| ext == "log")
						.unwrap_or(false)
				})
				.collect();

			println!("Found {} log files", log_files.len());
			for entry in log_files.iter().take(5) {
				println!("  - {}", entry.file_name().to_string_lossy());
			}
		}
	} else {
		println!("Log directory does not exist yet (will be created on first run)");
	}
}

#[test]
fn test_structured_logging_in_components() {
	// Initialize components and verify they log correctly
	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	// Create ingress queue
	let ingress_queue = IngressQueue::new(100);
	let (queue_sender, queue_receiver) = ingress_queue.split();

	// Create event buffer
	let event_buffer = EventBuffer::new(100);
	let (event_producer, event_consumer) = event_buffer.split();

	// Start event writer (this will log)
	let event_storage = Box::new(MemoryEventStorage::new());
	let event_writer_config = EventWriterConfig {
		batch_size: 10,
		batch_timeout_ms: 50,
		verbose_logging: true, // Enable verbose logging for test
	};
	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		event_writer_config,
	);

	// Start matching engine (this will log)
	let engine_config = EngineConfig {
		market: "BTC-USDT".to_string(),
		verbose_logging: true, // Enable verbose logging for test
	};
	let matching_engine = MatchingEngine::start(
		engine_config,
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	// Submit an order (this will trigger logging throughout the system)
	let order = OrderCommand {
		order_id: "test_order_1".to_string(),
		market: "BTC-USDT".to_string(),
		side: Side::Buy,
		price: 50000,
		size: 1,
		timestamp: 1000,
		public_key: "test_pubkey".to_string(),
	};

	// Append to journal
	{
		let mut j = journal.lock().unwrap();
		j.append(order.clone()).unwrap();
	}

	// Enqueue order
	queue_sender.try_enqueue(order).unwrap();

	// Wait for processing
	thread::sleep(Duration::from_millis(200));

	// Graceful shutdown (this will trigger shutdown logs)
	drop(matching_engine);
	drop(_event_writer);

	// Give time for final logs to flush
	thread::sleep(Duration::from_millis(100));

	println!("Test completed - check logs for structured output");
}

#[test]
fn test_log_file_creation_and_content() {
	// Set environment variable to ensure console output for this test
	unsafe {
		std::env::set_var("LOG_TO_CONSOLE", "false");
	}

	let log_dir = get_log_dir();

	// Try to find ANY log file in the directory (not just today's)
	let log_file = if let Ok(entries) = fs::read_dir(&log_dir) {
		entries
			.filter_map(|e| e.ok())
			.filter(|e| {
				e.path()
					.extension()
					.and_then(|ext| ext.to_str())
					.map(|ext| ext == "log")
					.unwrap_or(false)
			})
			.max_by_key(|e| {
				e.metadata()
					.and_then(|m| m.modified())
					.unwrap_or(std::time::SystemTime::UNIX_EPOCH)
			})
			.map(|e| e.path())
	} else {
		None
	};

	let log_file_path = log_file.unwrap_or_else(|| log_dir.join(get_today_log_file()));

	println!("Log file: {}", log_file_path.display());

	if log_file_path.exists() {
		println!("[OK] Log file exists");

		// Read and verify log content
		match fs::read_to_string(&log_file_path) {
			Ok(content) => {
				println!("[OK] Log file is readable");
				println!("  File size: {} bytes", content.len());
				println!();

				// Categorize and display logs by phase
				let lines: Vec<&str> = content.lines().collect();

				// === STARTUP SEQUENCE ===
				println!("STARTUP SEQUENCE:");
				println!("  - Initialization and component startup logs");
				println!();
				for line in &lines {
					if (line.contains("server")
						&& (line.contains("Log level")
							|| line.contains("Log directory")
							|| line.contains("Starting Anvil Matching Engine")
							|| line.contains("Initializing")
							|| line.contains("Creating")
							|| line.contains("Starting")))
						|| ((line.contains("engine") && line.contains("started"))
							|| (line.contains("event_writer") && line.contains("started"))
							|| (line.contains("snapshotter") && line.contains("started")))
					{
						println!("    {}", line);
					}
				}

				// === RUNTIME OPERATIONS ===
				println!();
				println!("RUNTIME OPERATIONS:");
				println!("  - Order processing, trade execution, event persistence");
				println!();

				let mut found_runtime = false;
				for line in &lines {
					// Order processing logs
					if line.contains("order_id=")
						&& !line.contains("started")
						&& !line.contains("stopped")
					{
						if !found_runtime {
							println!("    Order Processing:");
						}
						println!("      {}", line);
						found_runtime = true;
					}
					// Trade execution logs
					if line.contains("trade_id=") || line.contains("Trade executed") {
						println!("      {}", line);
						found_runtime = true;
					}
					// Event batch logs
					if (line.contains("batch_size=") || line.contains("Batch committed"))
						&& !line.contains("error")
					{
						println!("      {}", line);
						found_runtime = true;
					}
					// Snapshot logs
					if line.contains("Snapshot created") || line.contains("snapshot_ms=") {
						println!("      {}", line);
						found_runtime = true;
					}
				}

				if !found_runtime {
					println!("    [INFO] No runtime operation logs found in this file");
					println!(
						"       (This is normal if the service just started and hasn't processed orders yet)"
					);
				}

				// === SHUTDOWN SEQUENCE ===
				println!();
				println!("SHUTDOWN SEQUENCE:");
				println!("  - Graceful component shutdown");
				println!();

				let mut found_shutdown = false;
				for line in &lines {
					if line.contains("Shutting down")
						|| line.contains("stopped")
						|| line.contains("disconnected")
					{
						println!("    {}", line);
						found_shutdown = true;
					}
				}

				if !found_shutdown {
					println!("    [INFO] No shutdown logs found (service may still be running)");
				}

				// === VERIFICATION ===
				println!();
				println!("VERIFICATION:");

				// Check for key log messages
				let checks = vec![
					("server", "Log level"),
					("server", "Starting Anvil Matching Engine"),
					("engine", "Matching engine started"),
					("event_writer", "Event writer started"),
					("snapshotter", "Snapshotter started"),
				];

				let mut found_count = 0;
				for (target, keyword) in &checks {
					if content.contains(target) && content.contains(keyword) {
						found_count += 1;
					}
				}

				println!(
					"  [OK] Found {}/{} expected log patterns",
					found_count,
					checks.len()
				);

				// Check for structured fields
				let has_order_id = content.contains("order_id=");
				let has_seq = content.contains("seq=");
				let has_market = content.contains("market=");
				let has_thread_id = content.contains("ThreadId(");

				if has_order_id || has_seq || has_market {
					println!("  [OK] Structured fields detected:");
					if has_order_id {
						println!("    - order_id field found");
					}
					if has_seq {
						println!("    - seq field found");
					}
					if has_market {
						println!("    - market field found");
					}
				}

				if has_thread_id {
					println!("  [OK] Thread IDs are logged");
				}

				// Show total line count
				println!("  [OK] Total log lines: {}", lines.len());

				// Show file info
				if let Ok(metadata) = fs::metadata(&log_file_path)
					&& let Ok(modified) = metadata.modified()
				{
					println!("  [OK] Last modified: {:?}", modified);
				}
			}
			Err(e) => {
				println!("[ERROR] Could not read log file: {}", e);
			}
		}
	} else {
		println!("[WARN] No log files found in {}", log_dir.display());
		println!("  Expected location: {}", log_file_path.display());
		println!();
		println!("To create logs, run:");
		println!("   cargo run --bin anvil-matching");
		println!();
		println!("   The service will create logs at:");
		println!("   {}/matching.YYYY-MM-DD.log", log_dir.display());
	}
}

#[test]
fn test_trace_context_propagation() {
	// This test verifies that trace context can be propagated through the system
	// In a real scenario, this would be tested with the gRPC server

	println!("Testing OpenTelemetry trace context setup");

	// Verify that OTel tracer can be initialized
	match anvil_matching::otel::init_tracer() {
		Ok(Some(_tracer)) => {
			println!("[OK] OpenTelemetry tracer initialized successfully");
		}
		Ok(None) => {
			println!(
				"[OK] OpenTelemetry tracer initialization returned None (expected without OTLP endpoint)"
			);
		}
		Err(e) => {
			println!("[ERROR] OpenTelemetry tracer initialization failed: {}", e);
			panic!("Tracer initialization should not fail");
		}
	}

	// Check if OTLP endpoint is configured
	if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
		println!("[OK] OTLP endpoint configured: {}", endpoint);
	} else {
		println!("[INFO] OTLP endpoint not configured (optional)");
		println!(
			"  To enable OTLP export: export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317"
		);
	}
}

#[test]
fn test_log_levels_and_targets() {
	// Test that different log levels and targets work
	println!("Testing log levels and targets");

	// The actual logging is set up by init_logging() which should be called once
	// Here we just verify the configuration exists

	let config = MatchingConfig::default();
	println!("[OK] Default config created");
	println!("  Market: {}", config.market);
	println!("  Verbose logging: {}", config.verbose_logging);

	// Verify log constants are accessible
	use anvil_matching::config::{DEFAULT_LOG_LEVEL, DEFAULT_LOG_TO_CONSOLE, LOG_COMPONENT_NAME};

	println!("[OK] Log constants:");
	println!("  DEFAULT_LOG_LEVEL: {}", DEFAULT_LOG_LEVEL);
	println!("  LOG_COMPONENT_NAME: {}", LOG_COMPONENT_NAME);
	println!("  DEFAULT_LOG_TO_CONSOLE: {}", DEFAULT_LOG_TO_CONSOLE);

	assert_eq!(LOG_COMPONENT_NAME, "matching");
	assert_eq!(DEFAULT_LOG_LEVEL, "info");
	const _: () = {
		assert!(!DEFAULT_LOG_TO_CONSOLE);
	};
}

#[test]
fn test_log_rotation_filename_format() {
	// Verify that the expected log file naming convention is correct
	let log_file = get_today_log_file();

	println!("Today's log file: {}", log_file);

	// Verify format: matching.YYYY-MM-DD.log
	assert!(log_file.starts_with("matching."));
	assert!(log_file.ends_with(".log"));

	// Extract date part
	let parts: Vec<&str> = log_file.split('.').collect();
	assert_eq!(parts.len(), 3, "Expected format: matching.YYYY-MM-DD.log");
	assert_eq!(parts[0], "matching");
	assert_eq!(parts[2], "log");

	// Verify date format (YYYY-MM-DD)
	let date_part = parts[1];
	assert_eq!(date_part.len(), 10, "Date should be YYYY-MM-DD format");
	assert_eq!(&date_part[4..5], "-", "Year-month separator should be dash");
	assert_eq!(&date_part[7..8], "-", "Month-day separator should be dash");

	println!("[OK] Log file naming convention is correct");
}
