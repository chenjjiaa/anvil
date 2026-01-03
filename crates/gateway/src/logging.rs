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

//! Logging initialization for Gateway service
//!
//! This module provides logging configuration with file output and optional console output.
//!
//! # Configuration
//!
//! The following environment variables can be used to configure logging:
//!
//! - `RUST_LOG`: Log level filter (default: `info`)
//!   - Examples: `debug`, `info`, `warn`, `error`
//!   - Can be set per module: `RUST_LOG=anvil_gateway=debug,actix_web=info`
//!
//! - `LOG_DIR`: Root directory for log files (default: `{project_root}/logs`)
//!   - If not set, automatically detects project root and uses `logs/` subdirectory
//!   - Log files are created in `{LOG_DIR}/gateway/` directory
//!   - Example: `LOG_DIR=/var/log/anvil`
//!
//! - `LOG_TO_CONSOLE`: Enable console output (default: `false`)
//!   - Set to `true`, `1`, or `yes` to enable console output
//!   - When enabled, logs are output to both file and stderr
//!   - Console output includes ANSI colors for better readability
//!   - Example: `LOG_TO_CONSOLE=true`
//!
//! # Log File Format
//!
//! - Directory: `{LOG_DIR}/gateway/`
//! - Rotation: one file per day (UTC) using `tracing_appender::rolling::RollingFileAppender`
//! - Filename: `{component}.{date}.log` format (e.g., `gateway.2026-01-03.log`)
//! - Format: UTC timestamp, thread ID, log level, module path, message
//! - ANSI colors: Disabled in file output (enabled in console if `LOG_TO_CONSOLE=true`)

use std::{env, path::Path, sync::OnceLock};

use anyhow::{Context, Result};
use tracing::info;
use tracing_appender::{
	non_blocking,
	rolling::{self, Rotation},
};
use tracing_subscriber::{
	EnvFilter, fmt, layer::SubscriberExt, registry::Registry, util::SubscriberInitExt,
};

use crate::config::{DEFAULT_LOG_LEVEL, DEFAULT_LOG_TO_CONSOLE, LOG_COMPONENT_NAME};
use crate::otel;

// Store log guard to prevent log loss on program exit
static LOG_GUARD: OnceLock<non_blocking::WorkerGuard> = OnceLock::new();

/// Find project root directory by walking up from current location
///
/// Tries multiple strategies:
/// 1. Use CARGO_MANIFEST_DIR and walk up to find workspace root
/// 2. Walk up from current directory to find Cargo.toml
/// 3. Fallback to current directory
fn find_project_root() -> std::path::PathBuf {
	// Try to get project root from CARGO_MANIFEST_DIR (set by Cargo)
	if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
		let manifest_path = Path::new(&manifest_dir);
		// Walk up from crate directory to find workspace root (has Cargo.toml with [workspace])
		let mut current = manifest_path.to_path_buf();
		loop {
			let cargo_toml = current.join("Cargo.toml");
			if cargo_toml.exists() {
				// Check if it's a workspace root by reading Cargo.toml
				if let Ok(content) = std::fs::read_to_string(&cargo_toml)
					&& content.contains("[workspace]")
				{
					return current;
				}
			}
			if let Some(parent) = current.parent() {
				current = parent.to_path_buf();
			} else {
				break;
			}
		}
		// If workspace root not found, use crate directory's parent
		return manifest_path
			.parent()
			.map(|p| p.to_path_buf())
			.unwrap_or_else(|| manifest_path.to_path_buf());
	}

	// Fallback: try to find project root from current directory
	if let Ok(mut current_dir) = env::current_dir() {
		loop {
			if current_dir.join("Cargo.toml").exists() {
				return current_dir;
			}
			if let Some(parent) = current_dir.parent() {
				current_dir = parent.to_path_buf();
			} else {
				break;
			}
		}
	}

	// Last resort: use current directory
	env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
}

/// Get log root directory from environment or use project root/logs
///
/// # Returns
///
/// Returns the log root directory path as a string.
fn get_log_root() -> String {
	env::var("LOG_DIR").unwrap_or_else(|_| {
		let project_root = find_project_root();
		project_root.join("logs").to_string_lossy().to_string()
	})
}

/// Setup daily-rolling file logging layer.
///
/// `tracing-appender` handles the rotation, so long-running processes will
/// automatically switch files when the date changes.
///
/// Uses `RollingFileAppender::builder()` to configure a daily rolling appender
/// with `.log` suffix. This creates files in the format `{prefix}.{date}.log`,
/// e.g., `gateway.2026-01-03.log`.
fn setup_file_logging(log_dir: &Path) -> Result<non_blocking::NonBlocking> {
	// Daily rolling file appender in {LOG_DIR}/{component_name}/
	//
	// Use Builder API to configure filename prefix and suffix:
	// - prefix: component name (e.g., "gateway")
	// - suffix: ".log"
	// - rotation: daily
	// This creates files like "gateway.2026-01-03.log"
	let file_appender = rolling::RollingFileAppender::builder()
		.rotation(Rotation::DAILY)
		.filename_prefix(LOG_COMPONENT_NAME.to_string())
		.filename_suffix(".log")
		.build(log_dir)
		.with_context(|| {
			format!(
				"Failed to create rolling file appender in {}",
				log_dir.display()
			)
		})?;

	// Create non-blocking writer
	let (file_writer, guard) = non_blocking(file_appender);

	// Store guard to prevent log loss
	LOG_GUARD.set(guard).ok();

	Ok(file_writer)
}

/// Initialize logging with file output and optional console output
///
/// # Configuration
///
/// See module-level documentation for environment variable configuration.
///
/// # Returns
///
/// Returns `Ok(())` if logging is successfully initialized, or an error if
/// log directory or file cannot be created.
pub fn init_logging() -> Result<()> {
	dotenv::dotenv().ok();

	// Get log level from environment or use default
	let log_level = env::var("RUST_LOG").unwrap_or_else(|_| DEFAULT_LOG_LEVEL.to_string());

	// Get log root directory
	let log_root = get_log_root();

	// Create log directory structure: {LOG_DIR}/{component_name}/
	let log_dir = Path::new(&log_root).join(LOG_COMPONENT_NAME);
	std::fs::create_dir_all(&log_dir)
		.with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;

	// Setup file logging
	let file_writer = setup_file_logging(&log_dir)?;

	// Check if console output is enabled
	// Default: false (only file output)
	// Set LOG_TO_CONSOLE=true, LOG_TO_CONSOLE=1, or LOG_TO_CONSOLE=yes to enable
	let log_to_console = env::var("LOG_TO_CONSOLE")
		.map(|v| v == "true" || v == "1" || v == "yes")
		.unwrap_or(DEFAULT_LOG_TO_CONSOLE);

	// Initialize tracing filter from environment or default
	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));

	// Initialize OpenTelemetry tracer for distributed tracing
	// If OTLP endpoint is configured, traces are exported to external backend
	// Otherwise, traces are available locally via the tracing layer
	let otel_tracer = otel::init_tracer()?;

	if let Some(tracer) = otel_tracer {
		// Build subscriber with OpenTelemetry layer for distributed tracing
		// The OTel layer extracts trace context from spans and exports to OTLP (if configured)
		let subscriber = Registry::default()
			.with(filter.clone())
			.with(tracing_opentelemetry::layer().with_tracer(tracer));

		let subscriber = subscriber.with(
			fmt::layer()
				.with_writer(file_writer)
				.with_timer(fmt::time::UtcTime::rfc_3339())
				.with_thread_ids(true)
				.with_target(true)
				.with_thread_names(false)
				.with_ansi(false), // Disable ANSI colors for file output
		);

		if log_to_console {
			let subscriber = subscriber.with(
				fmt::layer()
					.with_writer(std::io::stderr)
					.with_timer(fmt::time::UtcTime::rfc_3339())
					.with_thread_ids(true)
					.with_target(true)
					.with_thread_names(false)
					.with_ansi(true),
			);
			subscriber.init();
		} else {
			subscriber.init();
		}
	} else {
		let subscriber = Registry::default().with(filter);

		let subscriber = subscriber.with(
			fmt::layer()
				.with_writer(file_writer)
				.with_timer(fmt::time::UtcTime::rfc_3339())
				.with_thread_ids(true)
				.with_target(true)
				.with_thread_names(false)
				.with_ansi(false),
		);

		if log_to_console {
			let subscriber = subscriber.with(
				fmt::layer()
					.with_writer(std::io::stderr)
					.with_timer(fmt::time::UtcTime::rfc_3339())
					.with_thread_ids(true)
					.with_target(true)
					.with_thread_names(false)
					.with_ansi(true),
			);
			subscriber.init();
		} else {
			subscriber.init();
		}
	}

	// Log initialization info
	info!(target: "server", "Log level: {}", log_level);
	info!(target: "server", "Log directory: {}", log_dir.display());
	info!(
		target: "server",
		"Log file base name: {}.YYYY-MM-DD.log (daily rolling)",
		LOG_COMPONENT_NAME
	);
	if log_to_console {
		info!(target: "server", "Console output: enabled");
	}

	Ok(())
}
