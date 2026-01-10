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

mod control;
mod state;

pub use control::EngineControlMessage;
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
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::{
	OrderBook,
	event::{EventProducer, MatchingEvent},
	journal::OrderJournal,
	queue::QueueReceiver,
	snapshot::{Snapshot, SnapshotMetadata},
	types::{Order, OrderCommand},
};

/// Result of a match operation including trade and maker order info
struct MatchResult {
	trade: Trade,
	maker_order_id: String,
	maker_was_fully_filled: bool,
	maker_remaining_size: u64,
}

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
///
/// Control messages (like snapshot requests) are handled via a separate
/// control channel to avoid blocking the matching loop.
pub struct MatchingEngine {
	thread_handle: Option<JoinHandle<()>>,
	shutdown: Arc<AtomicBool>,
	control_tx: mpsc::Sender<EngineControlMessage>,
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

		// Create control channel for snapshot requests and shutdown
		let (control_tx, control_rx) = mpsc::channel(16);

		let state = MatchingEngineState::new(config.market.clone());

		let thread_handle = thread::Builder::new()
			.name("matching-loop".to_string())
			.spawn(move || {
				info!(target: "engine", "Matching engine started for market: {}", config.market);
				// State is moved into the thread - complete ownership, no external Mutex
				Self::run_matching_loop(
					state,
					&config,
					&queue_receiver,
					&event_producer,
					&journal,
					control_rx,
					&shutdown_clone,
				);
				info!(target: "engine", "Matching engine stopped");
			})
			.expect("Failed to spawn matching engine thread");

		Self {
			thread_handle: Some(thread_handle),
			shutdown,
			control_tx,
		}
	}

	/// Main matching loop - the heart of the engine
	///
	/// This loop:
	/// 1. Dequeues OrderCommand from ingress queue (non-blocking with timeout)
	/// 2. Checks for control messages (snapshot requests, shutdown)
	/// 3. Applies matching logic with price-time priority
	/// 4. Emits events for all state changes
	/// 5. Updates in-memory orderbook
	fn run_matching_loop(
		mut state: MatchingEngineState,
		config: &EngineConfig,
		queue_receiver: &QueueReceiver,
		event_producer: &EventProducer,
		journal: &Arc<std::sync::Mutex<Box<dyn OrderJournal>>>,
		mut control_rx: mpsc::Receiver<EngineControlMessage>,
		shutdown: &Arc<AtomicBool>,
	) {
		loop {
			if shutdown.load(Ordering::Relaxed) {
				break;
			}

			// Check for control messages (non-blocking)
			match control_rx.try_recv() {
				Ok(EngineControlMessage::CreateSnapshot { respond_to }) => {
					// Create snapshot directly - we own the state
					let snapshot_result = Self::create_snapshot_internal(&state);
					let _ = respond_to.send(snapshot_result);
				}
				Ok(EngineControlMessage::RestoreSnapshot {
					snapshot,
					respond_to,
				}) => {
					// Restore state from snapshot
					let result = Self::restore_snapshot_internal(&mut state, snapshot);
					let _ = respond_to.send(result);
				}
				Ok(EngineControlMessage::ReplayEvents { events, respond_to }) => {
					// Replay events to rebuild state
					let result = Self::replay_events_internal(&mut state, events);
					let _ = respond_to.send(result);
				}
				Ok(EngineControlMessage::Shutdown) => {
					info!(target: "engine", "Received shutdown signal via control channel");
					break;
				}
				Err(mpsc::error::TryRecvError::Empty) => {
					// No control messages, continue with order processing
				}
				Err(mpsc::error::TryRecvError::Disconnected) => {
					warn!(target: "engine", "Control channel disconnected");
					break;
				}
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
					error!(target: "engine", "Ingress queue disconnected");
					break;
				}
				Err(crate::queue::QueueError::Full) => {
					// Should not happen on try_recv
					error!(target: "engine", "Unexpected Full error on try_recv");
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
			if let Err(e) = Self::process_order(&mut state, cmd.clone(), event_producer, journal) {
				error!(
					target: "engine",
					order_id = %cmd.order_id,
					error = %e,
					"Failed to process order"
				);
			}
		}
	}

	/// Process a single order command
	fn process_order(
		state: &mut MatchingEngineState,
		cmd: OrderCommand,
		event_producer: &EventProducer,
		_journal: &Arc<std::sync::Mutex<Box<dyn OrderJournal>>>,
	) -> Result<(), EngineError> {
		let order_size = cmd.size;
		let order_id = cmd.order_id.clone();
		let mut order: Order = cmd.clone().into();
		let mut trades = Vec::new();

		// Try to match the order
		while order.remaining_size > 0 {
			let match_result = match order.side {
				Side::Buy => Self::try_match_buy(&mut state.orderbook, &order),
				Side::Sell => Self::try_match_sell(&mut state.orderbook, &order),
			};

			match match_result {
				Some(result) => {
					let trade = result.trade.clone();
					order.remaining_size -= trade.size;

					// 1. Emit TradeExecuted event
					state.next_sequence += 1;
					debug!(
						order_id = %order_id,
						trade_id = %trade.trade_id,
						maker = %trade.maker_order_id,
						taker = %trade.taker_order_id,
						price = trade.price,
						size = trade.size,
						side = ?trade.side,
						seq = state.next_sequence,
						"Trade executed"
					);

					let trade_event = MatchingEvent::TradeExecuted {
						seq: state.next_sequence,
						trade: trade.clone(),
						timestamp: Self::timestamp(),
					};
					event_producer
						.push(trade_event)
						.map_err(|_| EngineError::EventBufferFull)?;

					// 2. Emit Maker order status event
					state.next_sequence += 1;
					let maker_event = if result.maker_was_fully_filled {
						debug!(
							maker_order_id = %result.maker_order_id,
							filled_size = trade.size,
							seq = state.next_sequence,
							"Maker order fully filled"
						);
						MatchingEvent::MakerOrderFilled {
							seq: state.next_sequence,
							order_id: result.maker_order_id.clone(),
							market: order.market.clone(),
							filled_size: trade.size,
							timestamp: Self::timestamp(),
						}
					} else {
						debug!(
							maker_order_id = %result.maker_order_id,
							filled_size = trade.size,
							remaining_size = result.maker_remaining_size,
							seq = state.next_sequence,
							"Maker order partially filled"
						);
						MatchingEvent::MakerOrderPartiallyFilled {
							seq: state.next_sequence,
							order_id: result.maker_order_id.clone(),
							market: order.market.clone(),
							filled_size: trade.size,
							remaining_size: result.maker_remaining_size,
							timestamp: Self::timestamp(),
						}
					};
					event_producer
						.push(maker_event)
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
			info!(
				order_id = %order_id,
				market = %order.market,
				side = ?order.side,
				filled_size = order_size,
				trades_count = trades.len(),
				seq = state.next_sequence,
				"Order fully filled"
			);

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

			// Note: mark_completed is now called by EventWriter after commit
		} else if !trades.is_empty() {
			// Partially filled
			let remaining_size = order.remaining_size;
			let filled_size = order_size - remaining_size;

			info!(
				order_id = %order_id,
				market = %order.market,
				side = ?order.side,
				filled_size = filled_size,
				remaining_size = remaining_size,
				trades_count = trades.len(),
				seq = state.next_sequence,
				"Order partially filled"
			);

			let event = MatchingEvent::OrderPartiallyFilled {
				seq: state.next_sequence,
				order_id: order.order_id.clone(),
				market: order.market.clone(),
				filled_size,
				remaining_size,
				timestamp: Self::timestamp(),
			};
			event_producer
				.push(event)
				.map_err(|_| EngineError::EventBufferFull)?;

			// Add remaining to orderbook
			state.orderbook.add_order(order);

			state.next_sequence += 1;

			debug!(
				order_id = %order_id,
				remaining_size = remaining_size,
				seq = state.next_sequence,
				"Order accepted to book"
			);

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

			debug!(
				order_id = %order_id,
				market = %cmd.market,
				side = ?cmd.side,
				price = cmd.price,
				size = remaining_size,
				seq = state.next_sequence,
				"Order accepted to book (no match)"
			);

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
	fn try_match_buy(orderbook: &mut OrderBook, taker_order: &Order) -> Option<MatchResult> {
		let best_ask = orderbook.best_ask()?;

		// Check if prices cross
		if taker_order.price < best_ask {
			return None;
		}

		let ask_level = orderbook.best_ask_level_mut()?;
		let maker_order = ask_level.get_first_order()?.clone();

		let match_price = maker_order.price;
		let match_size = taker_order.remaining_size.min(maker_order.remaining_size);
		let maker_was_fully_filled = maker_order.remaining_size == match_size;
		let maker_remaining_size = maker_order.remaining_size - match_size;

		// Update maker order
		if maker_was_fully_filled {
			// Maker fully filled, remove it
			ask_level.remove_first_order();
		} else {
			// Maker partially filled, update size
			ask_level.update_order_size(&maker_order.order_id, maker_remaining_size);
		}

		// Clean up empty level
		if ask_level.is_empty() {
			// Level will be removed automatically by BTreeMap entry API
		}

		let trade = Trade {
			trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
			market: taker_order.market.clone(),
			price: match_price,
			size: match_size,
			side: Side::Buy,
			timestamp: Self::timestamp(),
			maker_order_id: maker_order.order_id.clone(),
			taker_order_id: taker_order.order_id.clone(),
		};

		Some(MatchResult {
			trade,
			maker_order_id: maker_order.order_id,
			maker_was_fully_filled,
			maker_remaining_size,
		})
	}

	/// Try to match a sell order against the bid side
	fn try_match_sell(orderbook: &mut OrderBook, taker_order: &Order) -> Option<MatchResult> {
		let best_bid = orderbook.best_bid()?;

		// Check if prices cross
		if taker_order.price > best_bid {
			return None;
		}

		let bid_level = orderbook.best_bid_level_mut()?;
		let maker_order = bid_level.get_first_order()?.clone();

		let match_price = maker_order.price;
		let match_size = taker_order.remaining_size.min(maker_order.remaining_size);
		let maker_was_fully_filled = maker_order.remaining_size == match_size;
		let maker_remaining_size = maker_order.remaining_size - match_size;

		// Update maker order
		if maker_was_fully_filled {
			// Maker fully filled, remove it
			bid_level.remove_first_order();
		} else {
			// Maker partially filled, update size
			bid_level.update_order_size(&maker_order.order_id, maker_remaining_size);
		}

		let trade = Trade {
			trade_id: format!("trade_{}", uuid::Uuid::new_v4()),
			market: taker_order.market.clone(),
			price: match_price,
			size: match_size,
			side: Side::Sell,
			timestamp: Self::timestamp(),
			maker_order_id: maker_order.order_id.clone(),
			taker_order_id: taker_order.order_id.clone(),
		};

		Some(MatchResult {
			trade,
			maker_order_id: maker_order.order_id,
			maker_was_fully_filled,
			maker_remaining_size,
		})
	}

	fn timestamp() -> u64 {
		SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs()
	}

	/// Internal helper to create a snapshot from state (called within matching loop)
	fn create_snapshot_internal(state: &MatchingEngineState) -> Result<Snapshot, String> {
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

	/// Create a snapshot of current engine state
	///
	/// This method sends a snapshot request to the matching loop and waits for the response.
	/// It does not block the matching loop - the loop processes the request when convenient.
	pub fn create_snapshot(&self) -> Result<Snapshot, String> {
		// Create a oneshot channel for the response
		let (tx, rx) = oneshot::channel();

		// Send snapshot request via control channel
		self.control_tx
			.blocking_send(EngineControlMessage::CreateSnapshot { respond_to: tx })
			.map_err(|_| "Engine shut down or control channel full".to_string())?;

		// Wait for the response
		rx.blocking_recv()
			.map_err(|_| "Snapshot request cancelled or engine stopped".to_string())?
	}

	/// Restore engine state from a snapshot (internal helper)
	fn restore_snapshot_internal(
		state: &mut MatchingEngineState,
		snapshot: Snapshot,
	) -> Result<(), String> {
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

	/// Restore engine state from a snapshot (public API using control channel)
	pub fn restore_from_snapshot(&self, snapshot: Snapshot) -> Result<(), String> {
		let (tx, rx) = oneshot::channel();

		self.control_tx
			.blocking_send(EngineControlMessage::RestoreSnapshot {
				snapshot,
				respond_to: tx,
			})
			.map_err(|_| "Engine shut down or control channel full".to_string())?;

		rx.blocking_recv()
			.map_err(|_| "Restore request cancelled or engine stopped".to_string())?
	}

	/// Replay events to rebuild orderbook state (internal helper)
	///
	/// This is used during crash recovery to replay events from the
	/// last snapshot point to the current state.
	///
	/// Events are replayed in sequence order to reconstruct the exact orderbook state.
	/// Both taker and maker order state changes are handled.
	fn replay_events_internal(
		state: &mut MatchingEngineState,
		events: Vec<MatchingEvent>,
	) -> Result<(), String> {
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
					// Order was accepted and added to book
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
					// Taker order fully filled, remove from book
					// Try both sides since we don't know which side it was on
					let _ = state.orderbook.remove_order(Side::Buy, &order_id);
					let _ = state.orderbook.remove_order(Side::Sell, &order_id);
				}
				MatchingEvent::MakerOrderFilled { order_id, .. } => {
					// Maker order fully filled, remove from book
					let _ = state.orderbook.remove_order(Side::Buy, &order_id);
					let _ = state.orderbook.remove_order(Side::Sell, &order_id);
				}
				MatchingEvent::OrderPartiallyFilled {
					order_id,
					remaining_size,
					..
				} => {
					// Taker order partially filled
					// The order will be added to book with correct remaining_size
					// in a subsequent OrderAccepted event, so we can ignore this
					// or update if it's already in the book (edge case)
					if let Some(order) = state.orderbook.find_order_mut(&order_id) {
						order.remaining_size = remaining_size;
					}
				}
				MatchingEvent::MakerOrderPartiallyFilled {
					order_id,
					remaining_size,
					..
				} => {
					// Maker order partially filled, update size in book
					if let Some(order) = state.orderbook.find_order_mut(&order_id) {
						order.remaining_size = remaining_size;
					} else {
						warn!("Maker order {} not found in book during replay", order_id);
					}
				}
				MatchingEvent::OrderCancelled { order_id, .. } => {
					// Order cancelled, remove from book
					let _ = state.orderbook.remove_order(Side::Buy, &order_id);
					let _ = state.orderbook.remove_order(Side::Sell, &order_id);
				}
				MatchingEvent::TradeExecuted { .. } => {
					// TradeExecuted events are for audit/history
					// The actual state changes are captured in:
					// - MakerOrderPartiallyFilled / MakerOrderFilled
					// - OrderPartiallyFilled / OrderFilled
					// So we don't need to process TradeExecuted during replay
				}
				MatchingEvent::OrderRejected { .. } => {
					// Rejected orders never entered the book, no state change
				}
			}
		}

		info!("Event replay complete");
		Ok(())
	}

	/// Replay events to rebuild orderbook state (public API using control channel)
	pub fn replay_events(&self, events: Vec<MatchingEvent>) -> Result<(), String> {
		let (tx, rx) = oneshot::channel();

		self.control_tx
			.blocking_send(EngineControlMessage::ReplayEvents {
				events,
				respond_to: tx,
			})
			.map_err(|_| "Engine shut down or control channel full".to_string())?;

		rx.blocking_recv()
			.map_err(|_| "Replay request cancelled or engine stopped".to_string())?
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
