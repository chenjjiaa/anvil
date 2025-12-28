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

use crate::types::PlaceOrderRequest;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use k256::ecdsa::{
	Signature as EcdsaSignature, SigningKey as EcdsaSigningKey, VerifyingKey as EcdsaVerifyingKey,
};
use rand::rngs::OsRng;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::convert::TryInto;

/// Error types for signing operations
#[derive(Debug, thiserror::Error)]
pub enum SigningError {
	#[error("Serialization error: {0}")]
	Serialization(String),
	#[error("Signing error: {0}")]
	Signing(String),
	#[error("Invalid key: {0}")]
	InvalidKey(String),
	#[error("Unsupported algorithm: {0}")]
	UnsupportedAlgorithm(String),
}

/// Signature algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
	Ed25519,
	Ecdsa,
}

/// Sign an order request with Ed25519
pub fn sign_order_request_ed25519<T: Serialize>(
	request: &T,
	private_key: &[u8],
) -> Result<String, SigningError> {
	// Parse signing key
	let signing_key =
		SigningKey::from_bytes(private_key.try_into().map_err(|_| {
			SigningError::InvalidKey("Invalid Ed25519 private key length".to_string())
		})?);

	// Serialize the request
	let message = serialize_for_signing(request)?;

	// Sign
	let signature = signing_key.sign(&message);

	// Return hex-encoded signature
	Ok(hex::encode(signature.to_bytes()))
}

/// Sign an order request with ECDSA (secp256k1)
pub fn sign_order_request_ecdsa<T: Serialize>(
	request: &T,
	private_key: &[u8],
) -> Result<String, SigningError> {
	use k256::ecdsa::signature::Signer;

	// Parse signing key
	let signing_key = EcdsaSigningKey::from_bytes(private_key.into())
		.map_err(|e| SigningError::InvalidKey(format!("Invalid ECDSA private key: {}", e)))?;

	// Serialize the request
	let message = serialize_for_signing(request)?;

	// Hash message
	let message_hash = Sha256::digest(&message);

	// Sign
	let signature: EcdsaSignature = signing_key.sign(&message_hash[..]);

	// Return hex-encoded signature (compact format)
	let sig_bytes = signature.to_bytes();
	Ok(hex::encode(sig_bytes))
}

/// Sign an order request
///
/// This function serializes the order request and produces a signature.
/// The exact signing mechanism depends on the algorithm specified.
pub fn sign_order_request<T: Serialize>(
	request: &T,
	private_key: &[u8],
	algorithm: SignatureAlgorithm,
) -> Result<String, SigningError> {
	match algorithm {
		SignatureAlgorithm::Ed25519 => sign_order_request_ed25519(request, private_key),
		SignatureAlgorithm::Ecdsa => sign_order_request_ecdsa(request, private_key),
	}
}

/// Verify an order request signature with Ed25519
pub fn verify_order_signature_ed25519(
	request: &PlaceOrderRequest,
	signature: &str,
	public_key: &[u8],
) -> Result<bool, SigningError> {
	// Parse verifying key
	let verifying_key =
		VerifyingKey::from_bytes(public_key.try_into().map_err(|_| {
			SigningError::InvalidKey("Invalid Ed25519 public key length".to_string())
		})?)
		.map_err(|e| SigningError::InvalidKey(format!("Invalid Ed25519 public key: {}", e)))?;

	// Decode signature
	let sig_bytes = hex::decode(signature)
		.map_err(|e| SigningError::Signing(format!("Invalid hex signature: {}", e)))?;

	let signature = ed25519_dalek::Signature::from_bytes(
		&sig_bytes
			.try_into()
			.map_err(|_| SigningError::Signing("Invalid signature length".to_string()))?,
	);

	// Serialize request for signing
	let message = serialize_for_signing(request)?;

	// Verify
	Ok(verifying_key.verify(&message, &signature).is_ok())
}

/// Verify an order request signature with ECDSA
pub fn verify_order_signature_ecdsa(
	request: &PlaceOrderRequest,
	signature: &str,
	public_key: &[u8],
) -> Result<bool, SigningError> {
	use k256::ecdsa::signature::Verifier;

	// Parse verifying key
	let verifying_key = EcdsaVerifyingKey::from_sec1_bytes(public_key)
		.map_err(|e| SigningError::InvalidKey(format!("Invalid ECDSA public key: {}", e)))?;

	// Decode signature
	let sig_bytes = hex::decode(signature)
		.map_err(|e| SigningError::Signing(format!("Invalid hex signature: {}", e)))?;

	// Parse signature (DER format)
	let signature = EcdsaSignature::from_der(&sig_bytes)
		.map_err(|e| SigningError::Signing(format!("Invalid DER signature: {}", e)))?;

	// Serialize request for signing
	let message = serialize_for_signing(request)?;

	// Hash message
	let message_hash = Sha256::digest(&message);

	// Verify
	Ok(verifying_key.verify(&message_hash[..], &signature).is_ok())
}

/// Verify an order request signature
pub fn verify_order_signature(
	request: &PlaceOrderRequest,
	signature: &str,
	public_key: &[u8],
) -> Result<bool, SigningError> {
	// Try to detect algorithm from signature length
	let sig_bytes = hex::decode(signature)
		.map_err(|e| SigningError::Signing(format!("Invalid hex signature: {}", e)))?;

	match sig_bytes.len() {
		64 => {
			// Could be Ed25519 or ECDSA compact
			// Try Ed25519 first
			verify_order_signature_ed25519(request, signature, public_key)
				.or_else(|_| verify_order_signature_ecdsa(request, signature, public_key))
		}
		_ => verify_order_signature_ecdsa(request, signature, public_key),
	}
}

/// Serialize request for signing (canonical format)
fn serialize_for_signing<T: Serialize>(request: &T) -> Result<Vec<u8>, SigningError> {
	// Create a canonical representation for signing
	// This should match the gateway's verification format
	if let Ok(place_order) = serde_json::to_value(request)
		&& let Some(obj) = place_order.as_object()
	{
		let mut message = Vec::new();

		// Serialize in canonical order
		if let Some(market) = obj.get("market")
			&& let Some(s) = market.as_str()
		{
			message.extend_from_slice(s.as_bytes());
			message.push(0);
		}

		if let Some(side) = obj.get("side")
			&& let Some(s) = side.as_str()
		{
			match s {
				"buy" => message.push(0),
				"sell" => message.push(1),
				_ => {}
			}
		}

		if let Some(order_type) = obj.get("type")
			&& let Some(s) = order_type.as_str()
		{
			match s {
				"limit" => message.push(0),
				"market" => message.push(1),
				_ => {}
			}
		}

		if let Some(price) = obj.get("price")
			&& let Some(p) = price.as_u64()
		{
			message.extend_from_slice(&p.to_be_bytes());
		}

		if let Some(size) = obj.get("size")
			&& let Some(s) = size.as_u64()
		{
			message.extend_from_slice(&s.to_be_bytes());
		}

		if let Some(client_order_id) = obj.get("client_order_id")
			&& let Some(s) = client_order_id.as_str()
		{
			message.extend_from_slice(s.as_bytes());
		}

		return Ok(message);
	}

	// Fallback to JSON serialization
	serde_json::to_vec(request).map_err(|e| SigningError::Serialization(e.to_string()))
}

/// Generate a new Ed25519 keypair
pub fn generate_ed25519_keypair() -> (Vec<u8>, Vec<u8>) {
	let mut csprng = OsRng;
	let signing_key = SigningKey::generate(&mut csprng);
	let verifying_key = signing_key.verifying_key();
	(
		signing_key.to_bytes().to_vec(),
		verifying_key.to_bytes().to_vec(),
	)
}

/// Generate a new ECDSA keypair
pub fn generate_ecdsa_keypair() -> (Vec<u8>, Vec<u8>) {
	let signing_key = EcdsaSigningKey::random(&mut OsRng);
	let verifying_key = signing_key.verifying_key();
	let public_key_bytes = verifying_key.to_encoded_point(false);
	(
		signing_key.to_bytes().to_vec(),
		public_key_bytes.as_bytes().to_vec(),
	)
}
