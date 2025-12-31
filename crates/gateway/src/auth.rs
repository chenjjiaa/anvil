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

//! Cryptographic authentication for Gateway
//!
//! This module provides cryptographic signature verification for order requests.
//! Gateway only understands cryptographic identity (public keys and signatures),
//! NOT business user identity (user accounts, KYC, profiles, etc.).
//!
//! # Identity Model
//!
//! - **Principal**: Represents a cryptographic identity (public key + signature scheme)
//! - **AuthProvider**: Trait for extracting authentication materials from requests (pluggable)
//! - **AuthContext**: Protocol-agnostic container for authentication materials
//! - **Signature Verification**: Verifies that requests are signed by the private key holder
//!
//! Gateway does NOT understand:
//! - User accounts or user IDs
//! - KYC status
//! - User profiles or business-level identity
//! - Account balances (handled by settlement service)
//!
//! # Protocol Independence
//!
//! The authentication system is designed to be protocol-agnostic:
//!
//! - **AuthContext**: Abstracts away protocol-specific details (HTTP headers, gRPC metadata)
//! - **AuthProvider**: Works with AuthContext, not protocol-specific types
//! - **Same auth logic**: Works across HTTP, gRPC, WebSocket, and future protocols
//!
//! # Authentication Material Location (Protocol Requirement)
//!
//! **Authentication materials MUST be in request metadata, NOT in business payload.**
//!
//! This is a hard protocol requirement, not a recommendation:
//!
//! - **HTTP**: Public key in `X-Public-Key` header, signature in `X-Signature` header
//!   - Signature algorithm hint in `X-Signature-Alg` header (recommended for determinism)
//! - **gRPC**: Public key in `public-key` metadata, signature in `signature` metadata
//!   - Signature algorithm hint in `signature-alg` metadata (recommended for determinism)
//!
//! The order payload (`PlaceOrderRequest`) contains ONLY business data (market, price, size, etc.),
//! NEVER authentication materials. This strict separation ensures:
//!
//! - Clear semantic separation: Auth materials vs business payload
//! - Protocol determinism: No ambiguity about canonicalization
//! - Cross-protocol consistency: Same auth model across HTTP, gRPC, WebSocket

use anvil_sdk::types::PlaceOrderRequest;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use k256::ecdsa::{Signature as EcdsaSignature, VerifyingKey as EcdsaVerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Error types for authentication operations
#[derive(Debug, Error)]
pub enum AuthError {
	#[error("Invalid signature: {0}")]
	InvalidSignature(String),
	#[error("Missing signature")]
	MissingSignature,
	#[error("Invalid signature algorithm: {0}")]
	InvalidSignatureAlgorithm(String),
	#[error(
		"Unable to determine signature algorithm (provide X-Signature-Alg / signature-alg, or use a supported public key encoding)"
	)]
	UnableToDetermineAlgorithm,
	#[error("Missing timestamp")]
	MissingTimestamp,
	#[error("Invalid timestamp: {0}")]
	InvalidTimestamp(String),
	#[error("Missing nonce")]
	MissingNonce,
	#[error("Public key not found")]
	PublicKeyNotFound,
	#[error("Unsupported signature algorithm: {0}")]
	#[allow(dead_code)]
	UnsupportedAlgorithm(String),
	#[error("Signature format error: {0}")]
	SignatureFormatError(String),
}

/// Signature algorithm type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
	Ed25519,
	Ecdsa,
}

impl SignatureAlgorithm {
	/// Detect algorithm from signature length
	#[allow(dead_code)]
	pub fn detect(signature: &str) -> Result<Self, AuthError> {
		// Decode hex signature
		let sig_bytes = hex::decode(signature)
			.map_err(|e| AuthError::SignatureFormatError(format!("Invalid hex: {}", e)))?;

		match sig_bytes.len() {
			64 => {
				// Could be Ed25519 or ECDSA compact format
				// Try Ed25519 first (most common)
				Ok(SignatureAlgorithm::Ed25519)
			}
			65 => Ok(SignatureAlgorithm::Ecdsa), // ECDSA DER format
			_ => Err(AuthError::SignatureFormatError(format!(
				"Unknown signature length: {}",
				sig_bytes.len()
			))),
		}
	}
}

/// Cryptographic principal (identity)
///
/// Gateway only understands cryptographic identity, not business user identity.
/// A Principal represents a public key and its signature scheme.
///
/// This is the only identity concept that Gateway understands. It does not
/// understand user accounts, KYC, or business-level user IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
	/// Public key bytes
	pub public_key: Vec<u8>,
	/// Signature scheme/algorithm
	pub scheme: SignatureAlgorithm,
}

impl Principal {
	/// Create a new principal from public key and scheme
	pub fn new(public_key: Vec<u8>, scheme: SignatureAlgorithm) -> Self {
		Self { public_key, scheme }
	}

	/// Get principal identifier (hex-encoded public key)
	///
	/// This identifier uniquely identifies the cryptographic principal.
	/// It is NOT a business user ID.
	pub fn id(&self) -> String {
		hex::encode(&self.public_key)
	}
}

/// Authenticated principal plus protocol-level anti-replay metadata.
///
/// The `(timestamp, nonce)` pair is part of the signed message and must be
/// provided in request metadata (headers/metadata), not in the business payload.
pub struct AuthenticatedPrincipal {
	pub principal: Principal,
	pub timestamp: u64,
	pub nonce: String,
}

/// Authentication context - protocol-agnostic container for auth materials
///
/// This structure abstracts away protocol-specific details (HTTP headers,
/// gRPC metadata, WebSocket frames) and provides a unified interface
/// for extracting authentication materials.
///
/// Gateway adapters (HTTP handler, gRPC handler, etc.) construct this
/// from their protocol-specific sources, then pass it to AuthProvider.
///
/// # Protocol Specification
///
/// **Authentication materials MUST be transmitted in request metadata,
/// NOT in business payload.**
///
/// This is a hard protocol requirement, not a recommendation:
///
/// - **HTTP**: Public key in `X-Public-Key` header, signature in `X-Signature` header
/// - **gRPC**: Public key in `public-key` metadata, signature in `signature` metadata
///
/// Mixing authentication materials with business payload violates the
/// separation of concerns and creates protocol ambiguity.
///
/// # Fields
///
/// - `http_headers`: HTTP headers for HTTP/HTTPS requests
/// - `grpc_metadata`: gRPC metadata for gRPC requests
pub struct AuthContext<'a> {
	/// HTTP headers (for HTTP/HTTPS requests)
	/// Uses actix_web::http::header::HeaderMap directly since gateway already depends on actix-web
	pub http_headers: Option<&'a actix_web::http::header::HeaderMap>,

	/// gRPC metadata (for gRPC requests)
	/// Uses tonic::metadata::MetadataMap directly since gateway already depends on tonic
	pub grpc_metadata: Option<&'a tonic::metadata::MetadataMap>,
}

impl<'a> AuthContext<'a> {
	/// Create a new AuthContext for HTTP requests
	///
	/// Authentication materials must be in HTTP headers:
	/// - Public key: `X-Public-Key` header
	/// - Signature: `X-Signature` header
	/// - Timestamp: `X-Timestamp` header (unix seconds)
	/// - Nonce: `X-Nonce` header (opaque string)
	pub fn from_http(headers: &'a actix_web::http::header::HeaderMap) -> Self {
		Self {
			http_headers: Some(headers),
			grpc_metadata: None,
		}
	}

	/// Create a new AuthContext for gRPC requests
	///
	/// This will be used when implementing gRPC handlers in the future.
	///
	/// Authentication materials must be in gRPC metadata:
	/// - Public key: `public-key` metadata key
	/// - Signature: `signature` metadata key
	/// - Timestamp: `timestamp` metadata key (unix seconds)
	/// - Nonce: `nonce` metadata key (opaque string)
	#[allow(dead_code)]
	pub fn from_grpc(metadata: &'a tonic::metadata::MetadataMap) -> Self {
		Self {
			http_headers: None,
			grpc_metadata: Some(metadata),
		}
	}
}

/// Authentication provider trait
///
/// This allows gateway implementations to plug in their own authentication
/// mechanisms without modifying the SDK. Gateway only verifies cryptographic
/// signatures, not business user identity.
///
/// The gateway provides a default implementation (`SignatureAuthProvider`),
/// but production systems should implement their own provider based on their
/// authentication requirements (e.g., JWT tokens, API keys, etc.).
///
/// # Protocol Independence
///
/// This trait is protocol-agnostic. It receives `AuthContext` which abstracts
/// away protocol-specific details (HTTP headers, gRPC metadata, etc.).
/// This allows the same authentication logic to work across HTTP, gRPC,
/// WebSocket, and future protocols.
///
/// # Protocol Requirement
///
/// **Authentication materials MUST be extracted from request metadata
/// (headers/metadata), NOT from business payload body.**
///
/// This is a hard protocol requirement, not a recommendation. Implementations
/// that extract authentication materials from business payload violate the
/// protocol specification and create ambiguity.
pub trait AuthProvider: Send + Sync {
	/// Extract public key from authentication context
	///
	/// **Protocol Requirement**: Public key MUST be extracted from request metadata
	/// (HTTP headers or gRPC metadata), NOT from business payload body.
	///
	/// Examples:
	/// - From `X-Public-Key` HTTP header
	/// - From `public-key` gRPC metadata key
	/// - From JWT token claims in `Authorization` header
	///
	/// Implementations MUST NOT extract public key from business payload fields.
	fn extract_public_key(&self, ctx: &AuthContext) -> Result<Vec<u8>, AuthError>;

	/// Extract signature from authentication context
	///
	/// **Protocol Requirement**: Signature MUST be extracted from request metadata
	/// (HTTP headers or gRPC metadata), NOT from business payload body.
	///
	/// Default implementation must be provided by specific providers.
	/// Can be overridden for custom schemes (e.g., JWT-based auth).
	///
	/// Implementations MUST NOT extract signature from business payload fields.
	fn extract_signature(&self, ctx: &AuthContext) -> Result<String, AuthError>;

	/// Detect signature algorithm from authentication context
	///
	/// Default implementation prefers explicit metadata (if present), otherwise falls
	/// back to public key format/length (no guessing based on signature length).
	fn detect_algorithm(
		&self,
		ctx: &AuthContext,
		public_key: &[u8],
		_signature: &str,
	) -> Result<SignatureAlgorithm, AuthError> {
		if let Some(hint) = extract_signature_algorithm_hint(ctx)? {
			return parse_signature_algorithm_hint(&hint);
		}

		detect_algorithm_from_public_key(public_key)
	}
}

/// Default signature-based authentication provider
///
/// This implementation extracts public key and signature from request metadata
/// following the protocol specification:
///
/// - **HTTP**: Public key from `X-Public-Key` header, signature from `X-Signature` header
/// - **gRPC**: Public key from `public-key` metadata, signature from `signature` metadata
///
/// **Protocol Requirement**: Authentication materials MUST be in request metadata,
/// NOT in business payload body. This implementation strictly enforces this requirement.
pub struct SignatureAuthProvider;

impl AuthProvider for SignatureAuthProvider {
	fn extract_public_key(&self, ctx: &AuthContext) -> Result<Vec<u8>, AuthError> {
		// Try HTTP header (required location for HTTP requests)
		if let Some(headers) = ctx.http_headers
			&& let Some(pk_header) = headers.get("X-Public-Key")
		{
			let pk_str = pk_header
				.to_str()
				.map_err(|e| AuthError::SignatureFormatError(format!("Invalid header: {}", e)))?;
			return hex::decode(pk_str)
				.map_err(|e| AuthError::SignatureFormatError(format!("Invalid hex: {}", e)));
		}

		// Try gRPC metadata (required location for gRPC requests)
		if let Some(metadata) = ctx.grpc_metadata
			&& let Some(pk_val) = metadata.get("public-key")
		{
			let pk_str = pk_val
				.to_str()
				.map_err(|e| AuthError::SignatureFormatError(format!("Invalid metadata: {}", e)))?;
			return hex::decode(pk_str)
				.map_err(|e| AuthError::SignatureFormatError(format!("Invalid hex: {}", e)));
		}

		// Public key must be in request metadata, not in body
		Err(AuthError::PublicKeyNotFound)
	}

	fn extract_signature(&self, ctx: &AuthContext) -> Result<String, AuthError> {
		// Try HTTP header (required location for HTTP requests)
		if let Some(headers) = ctx.http_headers
			&& let Some(sig_header) = headers.get("X-Signature")
		{
			return sig_header
				.to_str()
				.map(|s| s.to_string())
				.map_err(|e| AuthError::SignatureFormatError(format!("Invalid header: {}", e)));
		}

		// Try gRPC metadata (required location for gRPC requests)
		if let Some(metadata) = ctx.grpc_metadata
			&& let Some(sig_val) = metadata.get("signature")
		{
			return sig_val
				.to_str()
				.map(|s| s.to_string())
				.map_err(|e| AuthError::SignatureFormatError(format!("Invalid metadata: {}", e)));
		}

		// Signature must be in request metadata, not in body
		Err(AuthError::MissingSignature)
	}

	fn detect_algorithm(
		&self,
		ctx: &AuthContext,
		public_key: &[u8],
		_signature: &str,
	) -> Result<SignatureAlgorithm, AuthError> {
		// Prefer explicit algorithm hint in metadata.
		if let Some(hint) = extract_signature_algorithm_hint(ctx)? {
			return parse_signature_algorithm_hint(&hint);
		}

		// Fallback: determine from public key (deterministic).
		//
		// We intentionally do NOT guess based on signature length because
		// Ed25519 and ECDSA (compact) can both be 64 bytes.
		detect_algorithm_from_public_key(public_key)
	}
}

fn extract_signature_algorithm_hint(ctx: &AuthContext) -> Result<Option<String>, AuthError> {
	// HTTP header
	if let Some(headers) = ctx.http_headers
		&& let Some(alg_header) = headers.get("X-Signature-Alg")
	{
		let alg_str = alg_header
			.to_str()
			.map_err(|e| AuthError::InvalidSignatureAlgorithm(format!("Invalid header: {}", e)))?;
		return Ok(Some(alg_str.to_string()));
	}

	// gRPC metadata
	if let Some(metadata) = ctx.grpc_metadata
		&& let Some(alg_val) = metadata.get("signature-alg")
	{
		let alg_str = alg_val.to_str().map_err(|e| {
			AuthError::InvalidSignatureAlgorithm(format!("Invalid metadata: {}", e))
		})?;
		return Ok(Some(alg_str.to_string()));
	}

	Ok(None)
}

fn parse_signature_algorithm_hint(hint: &str) -> Result<SignatureAlgorithm, AuthError> {
	let normalized = hint.trim().to_ascii_lowercase();
	match normalized.as_str() {
		"ed25519" => Ok(SignatureAlgorithm::Ed25519),
		"ecdsa" | "secp256k1" => Ok(SignatureAlgorithm::Ecdsa),
		other => Err(AuthError::InvalidSignatureAlgorithm(other.to_string())),
	}
}

fn detect_algorithm_from_public_key(public_key: &[u8]) -> Result<SignatureAlgorithm, AuthError> {
	// Deterministic heuristic based on well-known public key encodings:
	// - Ed25519 verifying key bytes: 32
	// - secp256k1 SEC1-encoded public key: 33 (compressed) or 65 (uncompressed)
	match public_key.len() {
		32 => Ok(SignatureAlgorithm::Ed25519),
		33 | 65 => Ok(SignatureAlgorithm::Ecdsa),
		_ => Err(AuthError::UnableToDetermineAlgorithm),
	}
}

fn extract_timestamp(ctx: &AuthContext) -> Result<u64, AuthError> {
	// HTTP header
	if let Some(headers) = ctx.http_headers
		&& let Some(ts_header) = headers.get("X-Timestamp")
	{
		let ts_str = ts_header
			.to_str()
			.map_err(|e| AuthError::InvalidTimestamp(format!("Invalid header: {}", e)))?;
		return ts_str
			.parse::<u64>()
			.map_err(|e| AuthError::InvalidTimestamp(format!("Invalid u64: {}", e)));
	}

	// gRPC metadata
	if let Some(metadata) = ctx.grpc_metadata
		&& let Some(ts_val) = metadata.get("timestamp")
	{
		let ts_str = ts_val
			.to_str()
			.map_err(|e| AuthError::InvalidTimestamp(format!("Invalid metadata: {}", e)))?;
		return ts_str
			.parse::<u64>()
			.map_err(|e| AuthError::InvalidTimestamp(format!("Invalid u64: {}", e)));
	}

	Err(AuthError::MissingTimestamp)
}

fn extract_nonce(ctx: &AuthContext) -> Result<String, AuthError> {
	// HTTP header
	if let Some(headers) = ctx.http_headers
		&& let Some(nonce_header) = headers.get("X-Nonce")
	{
		return nonce_header
			.to_str()
			.map(|s| s.to_string())
			.map_err(|e| AuthError::SignatureFormatError(format!("Invalid header: {}", e)));
	}

	// gRPC metadata
	if let Some(metadata) = ctx.grpc_metadata
		&& let Some(nonce_val) = metadata.get("nonce")
	{
		return nonce_val
			.to_str()
			.map(|s| s.to_string())
			.map_err(|e| AuthError::SignatureFormatError(format!("Invalid metadata: {}", e)));
	}

	Err(AuthError::MissingNonce)
}

/// Authenticate an order request by verifying its signature
///
/// This function verifies that the request was signed by the holder of the
/// private key corresponding to the given principal's public key.
///
/// Gateway only performs cryptographic verification - it does not understand
/// business user identity or account systems.
///
/// # Arguments
///
/// * `payload` - Order payload to verify signature against (business data only)
/// * `signature` - Signature extracted from request metadata (header/metadata)
/// * `principal` - Cryptographic principal containing public key and algorithm
pub fn authenticate_order(
	payload: &PlaceOrderRequest,
	signature: &str,
	principal: &Principal,
	timestamp: u64,
	nonce: &str,
) -> Result<(), AuthError> {
	if signature.is_empty() {
		return Err(AuthError::MissingSignature);
	}

	// Verify signature based on algorithm
	match principal.scheme {
		SignatureAlgorithm::Ed25519 => {
			verify_ed25519_signature(payload, signature, &principal.public_key, timestamp, nonce)
		}
		SignatureAlgorithm::Ecdsa => {
			verify_ecdsa_signature(payload, signature, &principal.public_key, timestamp, nonce)
		}
	}
}

/// Authenticate an order request using an AuthProvider
///
/// This function extracts authentication materials from AuthContext,
/// creates a Principal, and verifies the signature against the order payload.
///
/// # Arguments
///
/// * `ctx` - Authentication context containing protocol-specific auth materials
/// * `payload` - Order payload to verify signature against
/// * `provider` - Auth provider to extract auth materials from context
pub fn authenticate_with_provider(
	ctx: &AuthContext,
	payload: &PlaceOrderRequest,
	provider: &dyn AuthProvider,
) -> Result<AuthenticatedPrincipal, AuthError> {
	// Extract public key using provider
	let public_key = provider.extract_public_key(ctx)?;

	// Extract signature using provider
	let signature = provider.extract_signature(ctx)?;

	// Detect algorithm using provider
	let algorithm = provider.detect_algorithm(ctx, &public_key, &signature)?;

	// Extract anti-replay metadata (must be in request metadata)
	let timestamp = extract_timestamp(ctx)?;
	let nonce = extract_nonce(ctx)?;

	// Create principal
	let principal = Principal::new(public_key, algorithm);

	// Verify signature against payload
	// Signature is extracted from metadata (header/metadata), not from payload body
	authenticate_order(payload, &signature, &principal, timestamp, &nonce)?;

	Ok(AuthenticatedPrincipal {
		principal,
		timestamp,
		nonce,
	})
}

/// Verify Ed25519 signature
///
/// # Arguments
///
/// * `payload` - Order payload (business data only, no authentication materials)
/// * `signature` - Signature extracted from request metadata (hex-encoded)
/// * `public_key` - Public key bytes for verification
fn verify_ed25519_signature(
	payload: &PlaceOrderRequest,
	signature: &str,
	public_key: &[u8],
	timestamp: u64,
	nonce: &str,
) -> Result<(), AuthError> {
	// Parse verifying key
	let verifying_key = VerifyingKey::from_bytes(
		public_key
			.try_into()
			.map_err(|_| AuthError::PublicKeyNotFound)?,
	)
	.map_err(|e| AuthError::InvalidSignature(format!("Invalid Ed25519 public key: {}", e)))?;

	// Decode signature
	let sig_bytes = hex::decode(signature)
		.map_err(|e| AuthError::SignatureFormatError(format!("Invalid hex: {}", e)))?;

	let signature =
		Signature::from_bytes(&sig_bytes.try_into().map_err(|_| {
			AuthError::SignatureFormatError("Invalid signature length".to_string())
		})?);

	// Serialize payload for signing (business data only) + anti-replay metadata.
	let message = serialize_for_signing(payload, timestamp, nonce);

	// Verify signature
	verifying_key
		.verify(&message, &signature)
		.map_err(|e| AuthError::InvalidSignature(format!("Ed25519 verification failed: {}", e)))?;

	Ok(())
}

/// Verify ECDSA signature (secp256k1)
///
/// # Arguments
///
/// * `payload` - Order payload (business data only, no authentication materials)
/// * `signature` - Signature extracted from request metadata (hex-encoded)
/// * `public_key` - Public key bytes for verification
fn verify_ecdsa_signature(
	payload: &PlaceOrderRequest,
	signature: &str,
	public_key: &[u8],
	timestamp: u64,
	nonce: &str,
) -> Result<(), AuthError> {
	use k256::ecdsa::signature::Verifier;

	// Parse verifying key
	let verifying_key = EcdsaVerifyingKey::from_sec1_bytes(public_key)
		.map_err(|e| AuthError::InvalidSignature(format!("Invalid ECDSA public key: {}", e)))?;

	// Decode signature
	let sig_bytes = hex::decode(signature)
		.map_err(|e| AuthError::SignatureFormatError(format!("Invalid hex: {}", e)))?;

	// Parse signature (DER or compact format)
	let signature = if sig_bytes.len() == 64 {
		// Compact format (r || s)
		let sig_array: [u8; 64] = sig_bytes
			.try_into()
			.map_err(|_| AuthError::SignatureFormatError("Invalid signature length".to_string()))?;
		EcdsaSignature::from_bytes(&sig_array.into())
			.map_err(|e| AuthError::SignatureFormatError(format!("Invalid signature: {}", e)))?
	} else {
		// Try DER format
		EcdsaSignature::from_der(&sig_bytes)
			.map_err(|e| AuthError::SignatureFormatError(format!("Invalid DER signature: {}", e)))?
	};

	// Serialize payload for signing (business data only) + anti-replay metadata.
	let message = serialize_for_signing(payload, timestamp, nonce);

	// Hash message
	let message_hash = Sha256::digest(&message);

	// Verify signature
	verifying_key
		.verify(&message_hash[..], &signature)
		.map_err(|e| AuthError::InvalidSignature(format!("ECDSA verification failed: {}", e)))?;

	Ok(())
}

/// Serialize request payload for signing (canonical format)
///
/// This function creates a canonical representation of the business payload
/// for signature generation and verification.
///
/// **Protocol Requirement**:
/// - Business data is serialized from the request payload (`PlaceOrderRequest`)
/// - Anti-replay metadata (`timestamp`, `nonce`) is serialized from request metadata
/// - Authentication materials (signature, public key) are NOT included
fn serialize_for_signing(request: &PlaceOrderRequest, timestamp: u64, nonce: &str) -> Vec<u8> {
	// Canonical signing message:
	// - include business data from payload
	// - include (timestamp, nonce) from metadata to enable replay protection
	//
	// This must match the client's signing format exactly.
	let mut message = Vec::new();
	message.extend_from_slice(request.market.as_bytes());
	message.push(0);
	match request.side {
		anvil_sdk::types::Side::Buy => message.push(0),
		anvil_sdk::types::Side::Sell => message.push(1),
	}
	match request.order_type {
		anvil_sdk::types::OrderType::Limit => message.push(0),
		anvil_sdk::types::OrderType::Market => message.push(1),
	}
	if let Some(price) = request.price {
		message.extend_from_slice(&price.to_be_bytes());
	}
	message.extend_from_slice(&request.size.to_be_bytes());
	if let Some(ref client_order_id) = request.client_order_id {
		message.extend_from_slice(client_order_id.as_bytes());
	}

	// Separator before metadata fields
	message.push(0);
	message.extend_from_slice(&timestamp.to_be_bytes());
	message.push(0);
	message.extend_from_slice(nonce.as_bytes());

	message
}
