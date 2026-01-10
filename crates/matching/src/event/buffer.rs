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

use super::MatchingEvent;

/// SPSC Event Buffer for passing events from matching loop to event writer
///
/// This buffer decouples event production (matching loop) from event
/// persistence (event writer), allowing the matching loop to run at
/// full speed without blocking on I/O.
///
/// Properties:
/// - Single Producer (matching loop)
/// - Single Consumer (event writer)
/// - Bounded capacity for backpressure
/// - Non-blocking send (try_send returns error if full)
pub struct EventBuffer {
	sender: Sender<MatchingEvent>,
	receiver: Receiver<MatchingEvent>,
}

impl EventBuffer {
	/// Create a new event buffer with the specified capacity
	///
	/// Capacity should be tuned based on:
	/// - Expected event rate from matching loop
	/// - Event writer commit batch size
	/// - Acceptable memory usage
	pub fn new(capacity: usize) -> Self {
		let (sender, receiver) = bounded(capacity);
		Self { sender, receiver }
	}

	/// Split the buffer into producer and consumer ends
	///
	/// The producer end is used by the matching loop to push events.
	/// The consumer end is used by the event writer to pull events.
	pub fn split(self) -> (EventProducer, EventConsumer) {
		(
			EventProducer {
				sender: self.sender,
			},
			EventConsumer {
				receiver: self.receiver,
			},
		)
	}
}

/// Producer end of the event buffer (used by matching loop)
pub struct EventProducer {
	sender: Sender<MatchingEvent>,
}

impl EventProducer {
	/// Push an event to the buffer
	///
	/// Returns error if buffer is full, indicating backpressure.
	/// The matching loop should handle this by either:
	/// - Pausing order processing
	/// - Applying flow control to ingress
	/// - Logging and monitoring buffer pressure
	pub fn push(&self, event: MatchingEvent) -> Result<(), EventBufferError> {
		self.sender.try_send(event).map_err(|e| match e {
			TrySendError::Full(_) => EventBufferError::Full,
			TrySendError::Disconnected(_) => EventBufferError::Disconnected,
		})
	}

	/// Check if the buffer is full
	pub fn is_full(&self) -> bool {
		self.sender.is_full()
	}
}

/// Consumer end of the event buffer (used by event writer)
pub struct EventConsumer {
	receiver: Receiver<MatchingEvent>,
}

impl EventConsumer {
	/// Try to receive an event from the buffer (non-blocking)
	pub fn try_recv(&self) -> Result<MatchingEvent, EventBufferError> {
		self.receiver.try_recv().map_err(|e| match e {
			TryRecvError::Empty => EventBufferError::Empty,
			TryRecvError::Disconnected => EventBufferError::Disconnected,
		})
	}

	/// Receive an event from the buffer (blocking)
	pub fn recv(&self) -> Result<MatchingEvent, EventBufferError> {
		self.receiver
			.recv()
			.map_err(|_| EventBufferError::Disconnected)
	}

	/// Drain multiple events from the buffer (non-blocking)
	///
	/// Returns up to `max_count` events, or fewer if the buffer
	/// becomes empty. This is useful for batching events.
	pub fn drain(&self, max_count: usize) -> Vec<MatchingEvent> {
		let mut events = Vec::with_capacity(max_count);
		for _ in 0..max_count {
			match self.try_recv() {
				Ok(event) => events.push(event),
				Err(EventBufferError::Empty) => break,
				Err(EventBufferError::Disconnected) => break,
				Err(EventBufferError::Full) => break, // Should never happen on recv
			}
		}
		events
	}
}

/// Errors that can occur when interacting with the event buffer
#[derive(Debug, thiserror::Error)]
pub enum EventBufferError {
	#[error("Event buffer is full")]
	Full,
	#[error("Event buffer is empty")]
	Empty,
	#[error("Event buffer disconnected")]
	Disconnected,
}

#[cfg(test)]
mod tests {
	use super::*;
	use anvil_sdk::types::Side;

	fn create_test_event(seq: u64) -> MatchingEvent {
		MatchingEvent::OrderAccepted {
			seq,
			order_id: format!("order_{}", seq),
			market: "BTC-USDT".to_string(),
			side: Side::Buy,
			price: 50000,
			size: 1,
			timestamp: 1000,
		}
	}

	#[test]
	fn test_push_and_recv() {
		let buffer = EventBuffer::new(10);
		let (producer, consumer) = buffer.split();

		let event = create_test_event(1);
		producer.push(event.clone()).unwrap();

		let received = consumer.recv().unwrap();
		assert_eq!(received.sequence(), 1);
	}

	#[test]
	fn test_buffer_full() {
		let buffer = EventBuffer::new(2);
		let (producer, _consumer) = buffer.split();

		producer.push(create_test_event(1)).unwrap();
		producer.push(create_test_event(2)).unwrap();

		let result = producer.push(create_test_event(3));
		assert!(result.is_err());
		assert!(matches!(result, Err(EventBufferError::Full)));
	}

	#[test]
	fn test_drain() {
		let buffer = EventBuffer::new(10);
		let (producer, consumer) = buffer.split();

		for i in 0..5 {
			producer.push(create_test_event(i)).unwrap();
		}

		let drained = consumer.drain(10);
		assert_eq!(drained.len(), 5);

		let empty = consumer.drain(10);
		assert_eq!(empty.len(), 0);
	}
}
