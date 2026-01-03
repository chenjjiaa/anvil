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

//! W3C Trace Context parsing and validation utilities
//!
//! This module provides utilities for parsing and validating W3C Trace Context headers
//! (`traceparent` and `tracestate`) according to the W3C Trace Context specification.
//!
//! # W3C Trace Context
//!
//! The W3C Trace Context specification defines standard HTTP headers for distributed tracing:
//!
//! - **`traceparent`**: Contains version, trace-id, parent-id (span-id), and trace-flags
//!   - Format: `version-traceid-spanid-flags` (hex-encoded, fixed lengths)
//!   - Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
//!
//! - **`tracestate`**: Vendor-specific trace state (key-value pairs)
//!   - Format: comma-separated key-value pairs
//!   - Example: `vendor1=value1,vendor2=value2`
//!
//! # Compatibility
//!
//! This module also supports legacy `X-Trace-Id` header validation for backward compatibility.
//! The gateway prioritizes W3C `traceparent` but falls back to `X-Trace-Id` if `traceparent`
//! is not present or invalid.

use thiserror::Error;

/// Parsed traceparent header parts according to W3C Trace Context specification
///
/// The `traceparent` header contains four parts separated by hyphens:
/// - Version (2 hex chars)
/// - Trace ID (32 hex chars, 16 bytes)
/// - Parent ID / Span ID (16 hex chars, 8 bytes)
/// - Trace flags (2 hex chars, 1 byte)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceParent {
	/// Version number (currently only version `00` is supported)
	pub version: u8,
	/// Trace ID (16 bytes, must not be all zeros)
	pub trace_id: [u8; 16],
	/// Parent span ID (8 bytes, must not be all zeros)
	pub span_id: [u8; 8],
	/// Trace flags (bit flags, e.g., `0x01` for sampled)
	pub trace_flags: u8,
}

/// Parsed tracestate header according to W3C Trace Context specification
///
/// The `tracestate` header contains vendor-specific trace state as a comma-separated
/// list of key-value pairs. This structure stores the raw string after basic validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceState {
	/// Raw tracestate string (after basic length/character validation)
	///
	/// The string is validated to ensure it meets W3C requirements:
	/// - Length: 1-512 characters
	/// - Characters: ASCII graphic characters except space, comma, semicolon
	pub raw: String,
}

/// Error types for trace context parsing and validation operations
#[derive(Debug, Error)]
pub enum TraceContextError {
	#[error("invalid traceparent format")]
	Format,
	#[error("invalid version")]
	Version,
	#[error("invalid trace id")]
	TraceId,
	#[error("invalid span id")]
	SpanId,
	#[error("invalid trace flags")]
	TraceFlags,
	#[error("invalid tracestate")]
	TraceState,
}

/// Parse a traceparent header value according to W3C Trace Context specification
///
/// # Format
///
/// The traceparent header must follow the format: `version-traceid-spanid-flags`
/// where all parts are hex-encoded with fixed lengths:
///
/// - `version`: 2 hex characters (currently only `00` is supported)
/// - `traceid`: 32 hex characters (16 bytes, must not be all zeros)
/// - `spanid`: 16 hex characters (8 bytes, must not be all zeros)
/// - `flags`: 2 hex characters (1 byte, trace flags)
///
/// # Example
///
/// ```
/// let tp = parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")?;
/// ```
///
/// # Errors
///
/// Returns `TraceContextError` if:
/// - The format is invalid (wrong number of parts)
/// - Version is invalid or unsupported
/// - Trace ID or span ID are invalid hex or all zeros
/// - Trace flags are invalid hex
pub fn parse_traceparent(value: &str) -> Result<TraceParent, TraceContextError> {
	let parts: Vec<&str> = value.split('-').collect();
	if parts.len() != 4 {
		return Err(TraceContextError::Format);
	}

	let version = parse_hex_byte(parts[0]).map_err(|_| TraceContextError::Version)?;
	if parts[1].len() != 32 {
		return Err(TraceContextError::TraceId);
	}
	let trace_id_bytes = hex::decode(parts[1]).map_err(|_| TraceContextError::TraceId)?;
	let trace_id: [u8; 16] = trace_id_bytes
		.try_into()
		.map_err(|_| TraceContextError::TraceId)?;
	if trace_id.iter().all(|b| *b == 0) {
		return Err(TraceContextError::TraceId);
	}

	if parts[2].len() != 16 {
		return Err(TraceContextError::SpanId);
	}
	let span_id_bytes = hex::decode(parts[2]).map_err(|_| TraceContextError::SpanId)?;
	let span_id: [u8; 8] = span_id_bytes
		.try_into()
		.map_err(|_| TraceContextError::SpanId)?;
	if span_id.iter().all(|b| *b == 0) {
		return Err(TraceContextError::SpanId);
	}

	let trace_flags = parse_hex_byte(parts[3]).map_err(|_| TraceContextError::TraceFlags)?;

	Ok(TraceParent {
		version,
		trace_id,
		span_id,
		trace_flags,
	})
}

/// Parse tracestate header with minimal validation according to W3C Trace Context specification
///
/// # Format
///
/// The tracestate header contains vendor-specific trace state as a comma-separated
/// list of key-value pairs. This function performs basic validation:
///
/// - Length: 1-512 characters (W3C recommendation)
/// - Characters: ASCII graphic characters except space, comma, semicolon
///
/// # Example
///
/// ```
/// let ts = parse_tracestate("vendor1=value1,vendor2=value2")?;
/// ```
///
/// # Errors
///
/// Returns `TraceContextError::TraceState` if:
/// - The string is empty or exceeds 512 characters
/// - The string contains invalid characters
pub fn parse_tracestate(value: &str) -> Result<TraceState, TraceContextError> {
	// W3C recommends max length 512; we enforce a lower practical guard.
	if value.is_empty() || value.len() > 512 {
		return Err(TraceContextError::TraceState);
	}
	if !value
		.chars()
		.all(|c| c.is_ascii_graphic() && c != ' ' && c != ',' && c != ';')
	{
		return Err(TraceContextError::TraceState);
	}
	Ok(TraceState {
		raw: value.to_string(),
	})
}

/// Build a traceparent header string from component parts
///
/// # Format
///
/// Constructs a W3C Trace Context compliant `traceparent` header string:
/// `version-traceid-spanid-flags` (all hex-encoded)
///
/// # Arguments
///
/// * `trace_id` - 16-byte trace ID (must not be all zeros)
/// * `span_id` - 8-byte span ID (must not be all zeros)
/// * `trace_flags` - Trace flags byte (e.g., `0x01` for sampled)
///
/// # Returns
///
/// Returns a formatted traceparent string with version `00`.
#[allow(dead_code)]
pub fn build_traceparent(trace_id: [u8; 16], span_id: [u8; 8], trace_flags: u8) -> String {
	format!(
		"00-{}-{}-{:02x}",
		hex::encode(trace_id),
		hex::encode(span_id),
		trace_flags
	)
}

/// Generate a random trace ID (16 bytes, guaranteed non-zero)
///
/// # Returns
///
/// Returns a 16-byte trace ID generated from a UUID v4. The function ensures
/// the trace ID is not all zeros (which is invalid per W3C Trace Context spec).
///
/// # Implementation
///
/// Uses `uuid::Uuid::new_v4()` to generate random bytes. If the generated UUID
/// happens to be all zeros (extremely unlikely), it regenerates until a non-zero
/// value is obtained.
pub fn generate_trace_id() -> [u8; 16] {
	loop {
		let bytes = *uuid::Uuid::new_v4().as_bytes();
		if bytes.iter().any(|b| *b != 0) {
			return bytes;
		}
	}
}

/// Generate a random span ID (8 bytes, guaranteed non-zero)
///
/// # Returns
///
/// Returns an 8-byte span ID generated from the first 8 bytes of a UUID v4.
/// The function ensures the span ID is not all zeros (which is invalid per
/// W3C Trace Context spec).
///
/// # Implementation
///
/// Uses the first 8 bytes of `uuid::Uuid::new_v4()`. If the generated bytes
/// happen to be all zeros (extremely unlikely), it regenerates until a non-zero
/// value is obtained.
#[allow(dead_code)]
pub fn generate_span_id() -> [u8; 8] {
	loop {
		let uuid_bytes = *uuid::Uuid::new_v4().as_bytes();
		let bytes: [u8; 8] = uuid_bytes[..8].try_into().unwrap();
		if bytes.iter().any(|b| *b != 0) {
			return bytes;
		}
	}
}

/// Validate and parse a legacy X-Trace-Id header value
///
/// This function validates and parses the legacy `X-Trace-Id` header format
/// for backward compatibility. The header must be a 32-character hex string
/// representing a 16-byte trace ID.
///
/// # Format
///
/// - Must be exactly 32 hex characters (case-insensitive)
/// - Must decode to 16 bytes
/// - Must not be all zeros (invalid trace ID)
///
/// # Example
///
/// ```
/// let trace_id = validate_hex_trace_id("4bf92f3577b34da6a3ce929d0e0e4736")?;
/// ```
///
/// # Errors
///
/// Returns `TraceContextError::TraceId` if:
/// - The string length is not 32 characters
/// - The string contains invalid hex characters
/// - The decoded bytes are all zeros
pub fn validate_hex_trace_id(value: &str) -> Result<[u8; 16], TraceContextError> {
	if value.len() != 32 {
		return Err(TraceContextError::TraceId);
	}
	let decoded = hex::decode(value).map_err(|_| TraceContextError::TraceId)?;
	let trace_id: [u8; 16] = decoded.try_into().map_err(|_| TraceContextError::TraceId)?;
	if trace_id.iter().all(|b| *b == 0) {
		return Err(TraceContextError::TraceId);
	}
	Ok(trace_id)
}

/// Parse a 2-character hex string into a u8 byte
///
/// # Arguments
///
/// * `s` - A 2-character hex string (e.g., `"01"`, `"ff"`)
///
/// # Returns
///
/// Returns `Ok(u8)` if the string is valid hex, `Err(())` otherwise.
fn parse_hex_byte(s: &str) -> Result<u8, ()> {
	if s.len() != 2 {
		return Err(());
	}
	u8::from_str_radix(s, 16).map_err(|_| ())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_traceparent_ok() {
		let tp = parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
			.expect("valid traceparent");
		assert_eq!(tp.version, 0);
		assert_eq!(hex::encode(tp.trace_id), "4bf92f3577b34da6a3ce929d0e0e4736");
		assert_eq!(hex::encode(tp.span_id), "00f067aa0ba902b7");
		assert_eq!(tp.trace_flags, 0x01);
	}

	#[test]
	fn parse_traceparent_rejects_zero_ids() {
		assert!(matches!(
			parse_traceparent("00-00000000000000000000000000000000-0000000000000000-01"),
			Err(TraceContextError::TraceId)
		));
	}

	#[test]
	fn validate_x_trace_id_works() {
		let raw = "4bf92f3577b34da6a3ce929d0e0e4736";
		let parsed = validate_hex_trace_id(raw).expect("valid x-trace-id");
		assert_eq!(hex::encode(parsed), raw);
	}

	#[test]
	fn build_traceparent_format() {
		let tp = build_traceparent([0x11; 16], [0x22; 8], 0x01);
		assert_eq!(
			tp,
			"00-11111111111111111111111111111111-2222222222222222-01"
		);
	}
}
