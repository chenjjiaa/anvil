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

use anyhow::{Context, Result};

fn main() -> Result<()> {
	tonic_prost_build::configure()
		.build_server(true)
		.build_client(true)
		.compile_protos(&["proto/matching.proto"], &["proto/"])
		.context("Failed to compile matching.proto")?;

	// Also compile settlement proto for client use
	// In production, this would come from a shared proto package
	let settlement_proto = "../settlement/proto/settlement.proto";
	if std::path::Path::new(settlement_proto).exists() {
		tonic_prost_build::configure()
			.build_server(false)
			.build_client(true)
			.compile_protos(&[settlement_proto], &["../settlement/proto/"])
			.context("Failed to compile settlement.proto")?;
	}

	Ok(())
}
