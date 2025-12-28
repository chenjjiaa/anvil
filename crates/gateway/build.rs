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
	// Compile matching proto for client use
	let matching_proto = "../matching/proto/matching.proto";
	if std::path::Path::new(matching_proto).exists() {
		tonic_build::configure()
			.build_server(false)
			.build_client(true)
			.compile_protos(&[matching_proto], &["../matching/proto/"])
			.context("Failed to compile matching.proto")?;
	}
	Ok(())
}
