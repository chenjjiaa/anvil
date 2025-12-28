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

//! Anvil SDK - Client library for order submission
//!
//! This crate provides typed client interfaces for order submission,
//! shared request/response structures, and signing utilities.
//!
//! The SDK is designed to be lightweight and embeddable:
//! - No background threads
//! - No runtime initialization
//! - No environment or configuration loading

pub mod client;
pub mod signing;
pub mod types;

pub use client::{Client, SyncClient};
pub use signing::{SignatureAlgorithm, sign_order_request, verify_order_signature};
pub use types::*;
