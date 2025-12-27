// Copyright 2025 chenjjiaa
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

use anvil_sdk::signing::verify_order_signature;
use anvil_sdk::types::PlaceOrderRequest;
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
}

/// Authenticate an order request by verifying its signature
pub fn authenticate_order(request: &PlaceOrderRequest, public_key: &[u8]) -> Result<(), AuthError> {
	if request.signature.is_empty() {
		return Err(AuthError::MissingSignature);
	}

	if !verify_order_signature(request, &request.signature, public_key)
		.map_err(|e| AuthError::InvalidSignature(e.to_string()))?
	{
		return Err(AuthError::InvalidSignature(
			"Signature verification failed".to_string(),
		));
	}

	Ok(())
}

/// Extract user identifier from request (placeholder)
///
/// In a real implementation, this would extract the user ID from
/// the signature or authentication token.
pub fn extract_user_id(_request: &PlaceOrderRequest) -> String {
	// TODO: Extract from signature or JWT token
	"user_placeholder".to_string()
}
