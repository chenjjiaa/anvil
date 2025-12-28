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
	#[error("Public key not found")]
	PublicKeyNotFound,
	#[error("Unsupported signature algorithm: {0}")]
	#[allow(dead_code)]
	UnsupportedAlgorithm(String),
	#[error("Signature format error: {0}")]
	SignatureFormatError(String),
}

/// Signature algorithm type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
	Ed25519,
	Ecdsa,
}

impl SignatureAlgorithm {
	/// Detect algorithm from signature length
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

/// Authenticate an order request by verifying its signature
pub fn authenticate_order(request: &PlaceOrderRequest, public_key: &[u8]) -> Result<(), AuthError> {
	if request.signature.is_empty() {
		return Err(AuthError::MissingSignature);
	}

	// Detect signature algorithm
	let algorithm = SignatureAlgorithm::detect(&request.signature)?;

	// Verify signature based on algorithm
	match algorithm {
		SignatureAlgorithm::Ed25519 => verify_ed25519_signature(request, public_key),
		SignatureAlgorithm::Ecdsa => verify_ecdsa_signature(request, public_key),
	}
}

/// Verify Ed25519 signature
fn verify_ed25519_signature(
	request: &PlaceOrderRequest,
	public_key: &[u8],
) -> Result<(), AuthError> {
	// Parse verifying key
	let verifying_key = VerifyingKey::from_bytes(
		public_key
			.try_into()
			.map_err(|_| AuthError::PublicKeyNotFound)?,
	)
	.map_err(|e| AuthError::InvalidSignature(format!("Invalid Ed25519 public key: {}", e)))?;

	// Decode signature
	let sig_bytes = hex::decode(&request.signature)
		.map_err(|e| AuthError::SignatureFormatError(format!("Invalid hex: {}", e)))?;

	let signature =
		Signature::from_bytes(&sig_bytes.try_into().map_err(|_| {
			AuthError::SignatureFormatError("Invalid signature length".to_string())
		})?);

	// Serialize request for signing (excluding signature field)
	let message = serialize_for_signing(request);

	// Verify signature
	verifying_key
		.verify(&message, &signature)
		.map_err(|e| AuthError::InvalidSignature(format!("Ed25519 verification failed: {}", e)))?;

	Ok(())
}

/// Verify ECDSA signature (secp256k1)
fn verify_ecdsa_signature(request: &PlaceOrderRequest, public_key: &[u8]) -> Result<(), AuthError> {
	use k256::ecdsa::signature::Verifier;

	// Parse verifying key
	let verifying_key = EcdsaVerifyingKey::from_sec1_bytes(public_key)
		.map_err(|e| AuthError::InvalidSignature(format!("Invalid ECDSA public key: {}", e)))?;

	// Decode signature
	let sig_bytes = hex::decode(&request.signature)
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

	// Serialize request for signing
	let message = serialize_for_signing(request);

	// Hash message
	let message_hash = Sha256::digest(&message);

	// Verify signature
	verifying_key
		.verify(&message_hash[..], &signature)
		.map_err(|e| AuthError::InvalidSignature(format!("ECDSA verification failed: {}", e)))?;

	Ok(())
}

/// Serialize request for signing (canonical format)
fn serialize_for_signing(request: &PlaceOrderRequest) -> Vec<u8> {
	// Create a canonical representation for signing
	// This should match the client's signing format
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
	message
}

/// Extract user identifier from request
///
/// In a real implementation, this would extract the user ID from
/// the signature or authentication token.
pub fn extract_user_id(request: &PlaceOrderRequest) -> String {
	// TODO: Extract from signature or JWT token
	// For now, derive from public key hash
	if !request.signature.is_empty() {
		let sig_hash = Sha256::digest(request.signature.as_bytes());
		format!("user_{}", hex::encode(&sig_hash[..8]))
	} else {
		"user_anonymous".to_string()
	}
}

/// Extract public key from request headers or body
///
/// In production, this would extract from:
/// - Authorization header (JWT token)
/// - X-Public-Key header
/// - Request body
pub fn extract_public_key(_request: &PlaceOrderRequest) -> Result<Vec<u8>, AuthError> {
	// TODO: Extract from request headers or body
	// For now, return placeholder
	Err(AuthError::PublicKeyNotFound)
}
