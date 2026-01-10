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

//! DEPRECATED: Old multi-threaded matcher implementation
//!
//! This module contains the legacy multi-threaded matcher using DashMap.
//! It is kept for backwards compatibility but should not be used for new code.
//!
//! Use the new `engine` module instead, which provides a single-threaded
//! deterministic matching engine with proper crash recovery and event sourcing.

use crate::types::{MatchResult, MatchingError, Order};

/// DEPRECATED: Use crate::engine::MatchingEngine instead
#[deprecated(
	since = "0.1.0",
	note = "Use crate::engine::MatchingEngine instead for deterministic matching"
)]
pub struct Matcher;

#[allow(deprecated)]
impl Matcher {
	#[allow(dead_code)]
	pub fn new() -> Self {
		Self
	}

	#[allow(dead_code)]
	pub fn match_order(&self, _order: Order) -> Result<MatchResult, MatchingError> {
		Err(MatchingError::OrderBookError(
			"Legacy matcher is deprecated, use engine::MatchingEngine".to_string(),
		))
	}
}

#[allow(deprecated)]
impl Default for Matcher {
	fn default() -> Self {
		Self::new()
	}
}
