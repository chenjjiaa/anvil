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

use anvil_matching::types::OrderCommand;
use anvil_sdk::types::Side;

#[derive(Clone)]
pub enum Scenario {
	NoCross,
	CrossHeavy,
	DeepBook,
}

pub struct OrderGenerator {
	thread_id: usize,
	counter: u64,
	scenario: Scenario,
}

impl OrderGenerator {
	pub fn new(thread_id: usize, scenario: Scenario) -> Self {
		Self {
			thread_id,
			counter: 0,
			scenario,
		}
	}

	pub fn next_order(&mut self) -> OrderCommand {
		self.counter += 1;
		let order_id = format!("t{}-{}", self.thread_id, self.counter);

		match self.scenario {
			Scenario::NoCross => {
				if self.counter.is_multiple_of(2) {
					OrderCommand {
						order_id,
						market: "BTC-USDT".to_string(),
						side: Side::Buy,
						price: 44000 + (self.counter % 1000),
						size: 1,
						timestamp: now(),
						public_key: format!("bench_{}", self.thread_id),
					}
				} else {
					OrderCommand {
						order_id,
						market: "BTC-USDT".to_string(),
						side: Side::Sell,
						price: 56000 + (self.counter % 1000),
						size: 1,
						timestamp: now(),
						public_key: format!("bench_{}", self.thread_id),
					}
				}
			}
			Scenario::CrossHeavy => OrderCommand {
				order_id,
				market: "BTC-USDT".to_string(),
				side: if self.counter.is_multiple_of(2) {
					Side::Buy
				} else {
					Side::Sell
				},
				price: 50000,
				size: 10,
				timestamp: now(),
				public_key: format!("bench_{}", self.thread_id),
			},
			Scenario::DeepBook => {
				// “插针式扫深度”负载模型：
				// - 低频：发一个很大的 taker（用极端限价近似市价单扫盘）
				// - 高频：持续挂很多 maker，把簿堆深（双边、多档位）
				//
				// 注意：当前没有 IOC/Market 语义，因此 taker 吃不完的剩余会挂在簿上。
				let is_spike_taker = self.counter.is_multiple_of(100);

				if is_spike_taker {
					let side = if (self.counter / 100).is_multiple_of(2) {
						Side::Buy
					} else {
						Side::Sell
					};

					// 极端限价，确保跨多档成交（近似市价单）
					let price = match side {
						Side::Buy => 1_000_000_000,
						Side::Sell => 1,
					};

					OrderCommand {
						order_id,
						market: "BTC-USDT".to_string(),
						side,
						price,
						size: 10_000_000,
						timestamp: now(),
						public_key: format!("bench_{}", self.thread_id),
					}
				} else {
					let mid: u64 = 50_000;
					let levels: u64 = 2_000;
					let offset = (self.counter % levels) as i64 - (levels as i64 / 2);
					let price = (mid as i64 + offset) as u64;

					let side = if self.counter.is_multiple_of(2) {
						Side::Buy
					} else {
						Side::Sell
					};

					OrderCommand {
						order_id,
						market: "BTC-USDT".to_string(),
						side,
						price,
						size: 1_000,
						timestamp: now(),
						public_key: format!("bench_{}", self.thread_id),
					}
				}
			}
		}
	}

	pub fn warmup_orders(&self, count: usize) -> Vec<OrderCommand> {
		// 预热深度：双边、多档位，让“插针”更容易跨档位吃到大量流动性
		let mid: u64 = 50_000;
		let levels: u64 = 2_000;
		let half = levels / 2;

		(0..count)
			.map(|i| {
				let i = i as u64;
				let side = if i.is_multiple_of(2) {
					Side::Buy
				} else {
					Side::Sell
				};

				// buy < mid, sell > mid，避免预热单彼此成交
				let level = (i / 2) % half;
				let price = match side {
					Side::Buy => mid - 1 - level,
					Side::Sell => mid + 1 + level,
				};

				OrderCommand {
					order_id: format!("warmup-{}", i),
					market: "BTC-USDT".to_string(),
					side,
					price,
					size: 1_000,
					timestamp: now(),
					public_key: "warmup".to_string(),
				}
			})
			.collect()
	}
}

fn now() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_secs()
}
