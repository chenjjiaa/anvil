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

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anvil_matching::{
	EventBuffer, EventWriter, EventWriterConfig, IngressQueue, MatchingEngine, MemoryEventStorage,
	MemoryOrderJournal, OrderJournal, engine::EngineConfig,
};

mod common;
use common::metrics::BenchMetrics;
use common::order_generator::{OrderGenerator, Scenario};

const WARMUP_ORDERS: usize = 100_000;
const TEST_DURATION_SECS: u64 = 30;
const PRODUCER_COUNTS: &[usize] = &[1, 2, 4, 8, 16, 32];

fn benchmark_scenario(c: &mut Criterion, scenario_name: &str, scenario: Scenario) {
	let mut group = c.benchmark_group(scenario_name);
	group.sample_size(10);
	group.measurement_time(Duration::from_secs(TEST_DURATION_SECS + 10));

	for &producer_count in PRODUCER_COUNTS {
		group.bench_with_input(
			BenchmarkId::from_parameter(format!("{}p", producer_count)),
			&producer_count,
			|b, &num_producers| {
				b.iter_custom(|iters| {
					let mut total_duration = Duration::ZERO;

					for _ in 0..iters {
						total_duration += run_benchmark(num_producers, &scenario);
					}

					total_duration
				});
			},
		);
	}

	group.finish();
}

fn run_benchmark(num_producers: usize, scenario: &Scenario) -> Duration {
	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	let ingress_queue = IngressQueue::new(1_000_000);
	let (queue_sender, queue_receiver) = ingress_queue.split();

	let event_buffer = EventBuffer::new(1_000_000);
	let (event_producer, event_consumer) = event_buffer.split();

	let event_storage = Box::new(MemoryEventStorage::new());
	let event_writer_config = EventWriterConfig {
		batch_size: 1000,
		batch_timeout_ms: 50,
		verbose_logging: false,
	};

	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		event_writer_config,
	);

	let engine_config = EngineConfig {
		market: "BTC-USDT".to_string(),
		verbose_logging: false,
	};

	let _matching_engine = MatchingEngine::start(
		engine_config,
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	if matches!(scenario, Scenario::DeepBook) {
		let generator = OrderGenerator::new(0, Scenario::DeepBook);
		let warmup = generator.warmup_orders(WARMUP_ORDERS);
		for order in warmup {
			queue_sender.try_enqueue(order).ok();
		}
		thread::sleep(Duration::from_secs(2));
	}

	let metrics = BenchMetrics::new();
	let start = std::time::Instant::now();
	let mut handles = vec![];

	for i in 0..num_producers {
		let sender = queue_sender.clone();
		let metrics_clone = metrics.clone();
		let scenario_clone = scenario.clone();

		let handle = thread::spawn(move || {
			let mut generator = OrderGenerator::new(i, scenario_clone);
			let deadline = std::time::Instant::now() + Duration::from_secs(TEST_DURATION_SECS);

			while std::time::Instant::now() < deadline {
				let order = generator.next_order();

				// 阻塞式入队：失败时短暂退避重试，避免把队列打满
				loop {
					match sender.try_enqueue(order.clone()) {
						Ok(_) => {
							metrics_clone
								.enqueued
								.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
							break;
						}
						Err(_) => {
							metrics_clone
								.failed
								.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
							std::thread::sleep(std::time::Duration::from_micros(50));
						}
					}
				}
			}
		});

		handles.push(handle);
	}

	for handle in handles {
		handle.join().unwrap();
	}

	let duration = start.elapsed();

	thread::sleep(Duration::from_secs(2));

	let report = metrics.report();
	eprintln!(
		"[{}p] 入队: {} orders, 失败: {}, 吞吐: {:.0} orders/s",
		num_producers, report.total_enqueued, report.total_failed, report.throughput
	);

	duration
}

fn bench_no_cross(c: &mut Criterion) {
	benchmark_scenario(c, "no_cross", Scenario::NoCross);
}

fn bench_cross_heavy(c: &mut Criterion) {
	benchmark_scenario(c, "cross_heavy", Scenario::CrossHeavy);
}

fn bench_deep_book(c: &mut Criterion) {
	benchmark_scenario(c, "deep_book", Scenario::DeepBook);
}

criterion_group!(benches, bench_no_cross, bench_cross_heavy, bench_deep_book);
criterion_main!(benches);
