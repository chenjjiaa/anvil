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

use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::signal;
use tonic::transport::Server;

use anvil_matching::{
	EventBuffer, EventWriter, EventWriterConfig, IngressQueue, MatchingEngine, MemoryEventStorage,
	MemoryOrderJournal, OrderJournal, config::MatchingConfig, engine::EngineConfig, server,
};

#[tokio::main]
async fn main() -> Result<()> {
	unsafe {
		std::env::set_var("RUST_LOG", "error");
		std::env::set_var("LOG_TO_CONSOLE", "false");
	}

	let config = MatchingConfig::from_file("configs/bench.toml")
		.unwrap_or_else(|_| MatchingConfig::default());

	println!("Starting Benchmark Server");
	println!("Ingress Queue: {}", config.ingress_queue_size);
	println!("Event Buffer: {}", config.event_buffer_size);
	println!("Listening on: {}", config.bind_addr);

	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	let ingress_queue = IngressQueue::new(config.ingress_queue_size);
	let (queue_sender, queue_receiver) = ingress_queue.split();

	let event_buffer = EventBuffer::new(config.event_buffer_size);
	let (event_producer, event_consumer) = event_buffer.split();

	let event_storage = Box::new(MemoryEventStorage::new());
	let event_writer_config = EventWriterConfig {
		batch_size: config.event_batch_size,
		batch_timeout_ms: config.event_batch_timeout_ms,
		verbose_logging: false,
	};

	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		event_writer_config,
	);

	let engine_config = EngineConfig {
		market: config.market.clone(),
		verbose_logging: false,
	};

	let _matching_engine = MatchingEngine::start(
		engine_config,
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	let matching_service = server::create_server(queue_sender, journal, config.market.clone());

	println!("Server ready for benchmarking");

	Server::builder()
		.add_service(matching_service)
		.serve_with_shutdown(config.bind_addr, async {
			signal::ctrl_c().await.ok();
			println!("Shutting down...");
		})
		.await?;

	Ok(())
}
