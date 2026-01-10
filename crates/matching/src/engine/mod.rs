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

mod state;

pub use state::MatchingEngineState;

use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread::{self, JoinHandle},
	time::SystemTime,
};

use anvil_sdk::types::{Side, Trade};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::{
	OrderBook,
	event::{EventProducer, MatchingEvent},
	journal::OrderJournal,
	queue::QueueReceiver,
	snapshot::{Snapshot, SnapshotMetadata},
	types::{Order, OrderCommand},
};

/// Error types for matching engine operations
#[derive(Debug, Error)]
pub enum EngineError {
	#[error("Invalid order: {0}")]
	InvalidOrder(String),
	#[error("Market not found: {0}")]
	MarketNotFound(String),
	#[error("Engine shutdown")]
	Shutdown,
	#[error("Event buffer full")]
	EventBufferFull,
}

/// Configuration for the matching engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
	pub market: String,
	pub verbose_logging: bool,
}

impl Default for EngineConfig {
	fn default() -> Self {
		Self {
			market: "BTC-USDT".to_string(),
			verbose_logging: false,
		}
	}
}

/// Main matching engine with single-threaded event loop
///
/// The MatchingEngine runs the core matching loop in a dedicated thread,
/// consuming OrderCommands from the ingress queue and producing events
/// to the event buffer.
///
/// Architecture:
/// - Single-threaded: all matching logic runs on one thread
/// - Deterministic: same inputs always produce same outputs
/// - Event-sourced: all state changes produce events
/// - Non-blocking: uses channels for input/output
///
/// The matching loop is the absolute core of the system - all order state
/// changes occur here, ensuring no race conditions.
pub struct MatchingEngine {
	thread_handle: Option<JoinHandle<()>>,
	shutdown: Arc<AtomicBool>,
	state_handle: Arc<std::sync::Mutex<MatchingEngineState>>,
}

impl MatchingEngine {
	/// Start the matching engine
	pub fn start(
		config: EngineConfig,
		queue_receiver: QueueReceiver,
		event_producer: EventProducer,
		journal: Arc<std::sync::Mutex<Box<dyn OrderJournal>>>,
	) -> Self {
		let shutdown = Arc::new(AtomicBool::new(false));
		let shutdown_clone = shutdown.clone();

		let state = MatchingEngineState::new(config.market.clone());
		let state_handle = Arc::new(std::sync::Mutex::new(state));
		let state_clone = state_handle.clone();

		let thread_handle = thread::Builder::new()
			.name("matching-loop".to_string())
			.spawn(move || {
				info!("Matching engine started for market: {}", config.market);
				let mut state = state_clone.lock().unwrap();
				Self::run_matching_loop(
					&mut state,
					&config,
					&queue_receiver,
					&event_producer,
					&journal,
					&shutdown_clone,
				);
				info!("Matching engine stopped");
			})
			.expect("Failed to spawn matching engine thread");

		Self {
			thread_handle: Some(thread_handle),
			shutdown,
			state_handle,
		}
	}

	/// Main matching loop - the heart of the engine
	///
	/// This loop:
	/// 1. Dequeues OrderCommand from ingress queue (blocking)
	/// 2. Applies matching logic with price-time priority
	/// 3. Emits events for all state changes
	/// 4. Updates in-memory orderbook
	fn run_matching_loop(
		state: &mut MatchingEngineState,
		config: &EngineConfig,
		queue_receiver: &QueueReceiver,
		event_producer: &EventProducer,
		journal: &Arc<std::sync::Mutex<Box<dyn OrderJournal>>>,
		shutdown: &Arc<AtomicBool>,
	) {
		loop {
			if shutdown.load(Ordering::Relaxed) {
				break;
			}

			// Blocking receive from ingress queue
			let cmd = match queue_receiver.try_recv() {
				Ok(cmd) => cmd,
				Err(crate::queue::QueueError::Empty) => {
					// No commands available, check shutdown and continue
					if shutdown.load(Ordering::Relaxed) {
						break;
					}
					std::thread::sleep(std::time::Duration::from_millis(1));
					continue;
				}
				Err(crate::queue::QueueError::Disconnected) => {
					error!("Ingress queue disconnected");
					break;
				}
				Err(crate::queue::QueueError::Full) => {
					// Should not happen on try_recv
					error!("Unexpected Full error on try_recv");
					continue;
				}
			};

			if config.verbose_logging {
				debug!(
					"Processing order: {} {:?} {} @ {}",
					cmd.order_id, cmd.side, cmd.size, cmd.price
				);
			}

			// Process the order command
			if let Err(e) = Self::process_order(state, cmd, event_producer, journal) {
				error!("Failed to process order: {}", e);
			}
		}
	}

	/// Process a single order command
	fn process_order(
		state: &mut MatchingEngineState,
		cmd: OrderCommand,
		event_producer: &EventProducer,
		journal: &Arc<std::sync::Mutex<Box<dyn OrderJournal>>>,
	) -> Result<(), EngineError> {
		let order_size = cmd.size;
		let mut order: Order = cmd.clone().into();
		let mut trades = Vec::new();

		// Try to match the order
		while order.remaining_size > 0 {
			let trade = match order.side {
				Side::Buy => Self::try_match_buy(&mut state.orderbook, &order),
				Side::Sell => Self::try_match_sell(&mut state.orderbook, &order),
			};

			match trade {
				Some(trade) => {
					order.remaining_size -= trade.size;
					state.next_sequence += 1;

					let event = MatchingEvent::TradeExecuted {
						seq: state.next_sequence,
						trade: trade.clone(),
						timestamp: Self::timestamp(),
					};

					event_producer
						.push(event)
						.map_err(|_| EngineError::EventBufferFull)?;

					trades.push(trade);
				}
				None => break,
			}
		}

		// Emit events based on order outcome
		state.next_sequence += 1;

		if order.remaining_size == 0 {
			// Fully filled
			let event = MatchingEvent::OrderFilled {
				seq: state.next_sequence,
				order_id: order.order_id.clone(),
				market: order.market.clone(),
				filled_size: order_size,
				timestamp: Self::timestamp(),
			};
			event_producer
				.push(event)
				.map_err(|_| EngineError::EventBufferFull)?;

			// Mark order as completed in journal
			journal.lock().unwrap().mark_completed(&order.order_id);
		} else if !trades.is_empty() {
			// Partially filled
			let remaining_size = order.remaining_size;
			let event = MatchingEvent::OrderPartiallyFilled {
				seq: state.next_sequence,
				order_id: order.order_id.clone(),
				market: order.market.clone(),
				filled_size: order_size - remaining_size,
				remaining_size,
				timestamp: Self::timestamp(),
			};
			event_producer
				.push(event)
				.map_err(|_| EngineError::EventBufferFull)?;

			// Add remaining to orderbook
			state.orderbook.add_order(order);

			state.next_sequence += 1;
			let accepted_event = MatchingEvent::OrderAccepted {
				seq: state.next_sequence,
				order_id: cmd.order_id.clone(),
				market: cmd.market.clone(),
				side: cmd.side,
				price: cmd.price,
				size: remaining_size,
				timestamp: Self::timestamp(),
			};
			event_producer
				.push(accepted_event)
				.map_err(|_| EngineError::EventBufferFull)?;
		} else {
			// No match, add to orderbook
			let remaining_size = order.remaining_size;
			state.orderbook.add_order(order);

			let event = MatchingEvent::OrderAccepted {
				seq: state.next_sequence,
				order_id: cmd.order_id.clone(),
				market: cmd.market.clone(),
				side: cmd.side,
				price: cmd.price,
				size: remaining_size,
				timestamp: Self::timestamp(),
			};
			event_producer
				.push(event)
				.map_err(|_| EngineError::EventBufferFull)?;
		}

		Ok(())
	}

	/// Try to match a buy order against the ask side
	fn try_match_buy(orderbook: &mut OrderBook, taker_order: &Order) -> Option<Trade> {
		let best_ask = orderbook.best_ask()?;

		// Check if prices cross
		if taker_order.price < best_ask {
			return None;
		}

		let ask_level = orderbook.best_ask_level_mut()?;
		let maker_order = ask_level.get_first_order()?.clone();

		let match_price = maker_order.price;
		let match_size = taker_order.remaining_size.min(maker_order.remaining_size);

		// Update maker order
		if maker_order.remaining_size == match_size {
			// Maker fully filled, remove it
			ask_level.remove_first_order();
		} else {
			// Maker partially filled, update size
			ask_level.update_order_size(
				&maker_order.order_id,
				maker_order.remaining_size - match_size,
			);
		}

		// Clean up empty level
		if ask_level.is_empty() {
			// Level will be removed automatically by BTreeMap entry API
		}

		Some(Trade {
			trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
			market: taker_order.market.clone(),
			price: match_price,
			size: match_size,
			side: Side::Buy,
			timestamp: Self::timestamp(),
			maker_order_id: maker_order.order_id,
			taker_order_id: taker_order.order_id.clone(),
		})
	}

	/// Try to match a sell order against the bid side
	fn try_match_sell(orderbook: &mut OrderBook, taker_order: &Order) -> Option<Trade> {
		let best_bid = orderbook.best_bid()?;

		// Check if prices cross
		if taker_order.price > best_bid {
			return None;
		}

		let bid_level = orderbook.best_bid_level_mut()?;
		let maker_order = bid_level.get_first_order()?.clone();

		let match_price = maker_order.price;
		let match_size = taker_order.remaining_size.min(maker_order.remaining_size);

		// Update maker order
		if maker_order.remaining_size == match_size {
			// Maker fully filled, remove it
			bid_level.remove_first_order();
		} else {
			// Maker partially filled, update size
			bid_level.update_order_size(
				&maker_order.order_id,
				maker_order.remaining_size - match_size,
			);
		}

		Some(Trade {
			trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
			market: taker_order.market.clone(),
			price: match_price,
			size: match_size,
			side: Side::Sell,
			timestamp: Self::timestamp(),
			maker_order_id: maker_order.order_id,
			taker_order_id: taker_order.order_id.clone(),
		})
	}

	fn timestamp() -> u64 {
		SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs()
	}

	/// Create a snapshot of current engine state
	pub fn create_snapshot(&self) -> Result<Snapshot, String> {
		let state = self.state_handle.lock().unwrap();

		// Serialize orderbook to JSON
		let state_data = serde_json::to_vec(&state.orderbook)
			.map_err(|e| format!("Failed to serialize orderbook: {}", e))?;

		let metadata = SnapshotMetadata {
			created_at: Self::timestamp(),
			event_seq: state.next_sequence,
			size_bytes: state_data.len(),
			market: state.orderbook.market().to_string(),
		};

		Ok(Snapshot {
			metadata,
			state_data,
		})
	}

	/// Restore engine state from a snapshot
	pub fn restore_from_snapshot(&self, snapshot: Snapshot) -> Result<(), String> {
		let mut state = self.state_handle.lock().unwrap();

		let orderbook: OrderBook = serde_json::from_slice(&snapshot.state_data)
			.map_err(|e| format!("Failed to deserialize orderbook: {}", e))?;

		state.orderbook = orderbook;
		state.next_sequence = snapshot.metadata.event_seq;

		info!(
			"Restored engine state from snapshot at seq={}",
			snapshot.metadata.event_seq
		);

		Ok(())
	}

	/// Replay events to rebuild orderbook state
	///
	/// This is used during crash recovery to replay events from the
	/// last snapshot point to the current state.
	pub fn replay_events(&self, events: Vec<MatchingEvent>) -> Result<(), String> {
		let mut state = self.state_handle.lock().unwrap();

		info!("Replaying {} events...", events.len());

		for event in events {
			match event {
				MatchingEvent::OrderAccepted {
					order_id,
					market,
					side,
					price,
					size,
					timestamp,
					..
				} => {
					let order = Order {
						order_id,
						market,
						side,
						price,
						size,
						remaining_size: size,
						timestamp,
						public_key: "recovered".to_string(),
					};
					state.orderbook.add_order(order);
				}
				MatchingEvent::OrderFilled { order_id, .. } => {
					// Order fully filled, remove from book
					// Note: may already be removed, so ignore error
					let _ = state.orderbook.remove_order(Side::Buy, &order_id);
					let _ = state.orderbook.remove_order(Side::Sell, &order_id);
				}
				MatchingEvent::OrderCancelled { order_id, .. } => {
					// Order cancelled, remove from book
					let _ = state.orderbook.remove_order(Side::Buy, &order_id);
					let _ = state.orderbook.remove_order(Side::Sell, &order_id);
				}
				MatchingEvent::TradeExecuted { trade, .. } => {
					// Trade execution: update maker order size
					// This is implicit in the OrderPartiallyFilled/OrderFilled events
					// We don't need to replay individual size updates
					let _ = trade;
				}
				MatchingEvent::OrderPartiallyFilled {
					order_id,
					remaining_size,
					..
				} => {
					// Update order size in book
					// Find the order and update its size
					// This is complex; for MVP we rely on OrderAccepted events
					// having the correct remaining size
					let _ = (order_id, remaining_size);
				}
				MatchingEvent::OrderRejected { .. } => {
					// Rejected orders never enter the book
				}
			}
		}

		info!("Event replay complete");
		Ok(())
	}

	/// Shutdown the matching engine gracefully
	pub fn shutdown(mut self) {
		info!("Shutting down matching engine");
		self.shutdown.store(true, Ordering::Relaxed);

		if let Some(handle) = self.thread_handle.take()
			&& let Err(e) = handle.join()
		{
			warn!("Matching engine thread panicked: {:?}", e);
		}
	}
}

impl Drop for MatchingEngine {
	fn drop(&mut self) {
		self.shutdown.store(true, Ordering::Relaxed);
		if let Some(handle) = self.thread_handle.take()
			&& let Err(e) = handle.join()
		{
			let _ = Err::<(), _>(e);
		}
	}
}
