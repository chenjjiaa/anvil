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

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
pub struct BenchMetrics {
	pub enqueued: Arc<AtomicU64>,
	pub failed: Arc<AtomicU64>,
	pub start_time: std::time::Instant,
}

impl BenchMetrics {
	pub fn new() -> Self {
		Self {
			enqueued: Arc::new(AtomicU64::new(0)),
			failed: Arc::new(AtomicU64::new(0)),
			start_time: std::time::Instant::now(),
		}
	}

	pub fn report(&self) -> BenchReport {
		let elapsed = self.start_time.elapsed().as_secs_f64();
		let enqueued = self.enqueued.load(Ordering::Relaxed);
		let failed = self.failed.load(Ordering::Relaxed);

		BenchReport {
			total_enqueued: enqueued,
			total_failed: failed,
			elapsed_secs: elapsed,
			throughput: enqueued as f64 / elapsed,
		}
	}
}

pub struct BenchReport {
	pub total_enqueued: u64,
	pub total_failed: u64,
	#[allow(dead_code)]
	pub elapsed_secs: f64,
	pub throughput: f64,
}
