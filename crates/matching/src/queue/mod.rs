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

use crossbeam::channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};

use crate::types::OrderCommand;

/// Ingress Queue abstraction for passing orders from RPC layer to matching loop
///
/// The Ingress Queue serves as the boundary between the multi-threaded
/// RPC ingress layer and the single-threaded matching loop. It provides
/// a deterministic ordering of commands entering the matching engine.
///
/// Properties:
/// - Multiple Producers (RPC handlers across threads)
/// - Single Consumer (matching loop)
/// - Bounded capacity for backpressure
/// - Explicit failure semantics when full
///
/// The queue does NOT:
/// - Provide scheduling or prioritization
/// - Make business decisions about order acceptance
/// - Implement retry logic
///
/// When the queue is full, it signals backpressure to the RPC layer,
/// which should reject new orders with OVERLOADED status.
pub struct IngressQueue {
	sender: Sender<OrderCommand>,
	receiver: Receiver<OrderCommand>,
}

impl IngressQueue {
	/// Create a new ingress queue with the specified capacity
	///
	/// Capacity should be tuned based on:
	/// - Expected order arrival rate
	/// - Matching loop processing rate
	/// - Acceptable latency for backpressure
	pub fn new(capacity: usize) -> Self {
		let (sender, receiver) = bounded(capacity);
		Self { sender, receiver }
	}

	/// Split the queue into sender and receiver ends
	///
	/// The sender can be cloned for multiple RPC threads.
	/// The receiver must remain unique for the single matching loop.
	pub fn split(self) -> (QueueSender, QueueReceiver) {
		(
			QueueSender {
				sender: self.sender,
			},
			QueueReceiver {
				receiver: self.receiver,
			},
		)
	}
}

/// Sender end of the ingress queue (used by RPC handlers)
///
/// This can be cloned and shared across multiple threads.
#[derive(Clone)]
pub struct QueueSender {
	sender: Sender<OrderCommand>,
}

impl QueueSender {
	/// Try to enqueue an order command (non-blocking)
	///
	/// Returns error if the queue is full, indicating that the
	/// matching engine is overloaded and cannot accept new orders.
	pub fn try_enqueue(&self, cmd: OrderCommand) -> Result<(), QueueError> {
		self.sender.try_send(cmd).map_err(|e| match e {
			TrySendError::Full(_) => QueueError::Full,
			TrySendError::Disconnected(_) => QueueError::Disconnected,
		})
	}

	/// Check if the queue is full
	pub fn is_full(&self) -> bool {
		self.sender.is_full()
	}
}

/// Receiver end of the ingress queue (used by matching loop)
///
/// This should NOT be cloned - only one matching loop should consume.
pub struct QueueReceiver {
	receiver: Receiver<OrderCommand>,
}

impl QueueReceiver {
	/// Receive an order command (blocking)
	///
	/// This is the main method used by the matching loop to dequeue
	/// the next command. It blocks until a command is available.
	pub fn recv(&self) -> Result<OrderCommand, QueueError> {
		self.receiver.recv().map_err(|_| QueueError::Disconnected)
	}

	/// Try to receive an order command (non-blocking)
	///
	/// Useful for implementing graceful shutdown or polling-based loops.
	pub fn try_recv(&self) -> Result<OrderCommand, QueueError> {
		self.receiver.try_recv().map_err(|e| match e {
			TryRecvError::Empty => QueueError::Empty,
			TryRecvError::Disconnected => QueueError::Disconnected,
		})
	}
}

/// Errors that can occur when interacting with the ingress queue
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
	#[error("Queue is full")]
	Full,
	#[error("Queue is empty")]
	Empty,
	#[error("Queue disconnected")]
	Disconnected,
}

#[cfg(test)]
mod tests {
	use super::*;
	use anvil_sdk::types::Side;

	fn create_test_command(order_id: &str) -> OrderCommand {
		OrderCommand {
			order_id: order_id.to_string(),
			market: "BTC-USDT".to_string(),
			side: Side::Buy,
			price: 50000,
			size: 1,
			timestamp: 1000,
			public_key: "test_key".to_string(),
		}
	}

	#[test]
	fn test_enqueue_and_recv() {
		let queue = IngressQueue::new(10);
		let (sender, receiver) = queue.split();

		let cmd = create_test_command("order_1");
		sender.try_enqueue(cmd.clone()).unwrap();

		let received = receiver.recv().unwrap();
		assert_eq!(received.order_id, "order_1");
	}

	#[test]
	fn test_queue_full() {
		let queue = IngressQueue::new(2);
		let (sender, _receiver) = queue.split();

		sender.try_enqueue(create_test_command("order_1")).unwrap();
		sender.try_enqueue(create_test_command("order_2")).unwrap();

		let result = sender.try_enqueue(create_test_command("order_3"));
		assert!(result.is_err());
		assert!(matches!(result, Err(QueueError::Full)));
	}

	#[test]
	fn test_multiple_senders() {
		let queue = IngressQueue::new(10);
		let (sender, receiver) = queue.split();

		let sender1 = sender.clone();
		let sender2 = sender.clone();

		sender1.try_enqueue(create_test_command("order_1")).unwrap();
		sender2.try_enqueue(create_test_command("order_2")).unwrap();

		let received1 = receiver.recv().unwrap();
		let received2 = receiver.recv().unwrap();

		assert!(received1.order_id == "order_1" || received1.order_id == "order_2");
		assert!(received2.order_id == "order_1" || received2.order_id == "order_2");
		assert_ne!(received1.order_id, received2.order_id);
	}
}
