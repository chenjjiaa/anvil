use std::sync::{Arc, Mutex};

use anvil_sdk::types::Side;

use anvil_matching::{
	EventBuffer, EventStorage, EventWriter, EventWriterConfig, IngressQueue, MatchingEngine,
	MemoryEventStorage, MemoryOrderJournal, OrderJournal, engine::EngineConfig,
	event::MatchingEvent, types::OrderCommand,
};

#[test]
fn test_crash_recovery_with_event_replay() {
	// Phase 1: Create initial state and process some orders
	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	let ingress_queue = IngressQueue::new(100);
	let (queue_sender, queue_receiver) = ingress_queue.split();

	let event_buffer = EventBuffer::new(100);
	let (event_producer, event_consumer) = event_buffer.split();

	let event_storage = Box::new(MemoryEventStorage::new());

	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		EventWriterConfig {
			batch_size: 10,
			batch_timeout_ms: 100,
			verbose_logging: true,
		},
	);

	let engine_config = EngineConfig {
		market: "BTC-USDT".to_string(),
		verbose_logging: true,
	};

	let matching_engine = MatchingEngine::start(
		engine_config.clone(),
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	// Submit orders
	let buy_order = OrderCommand {
		order_id: "order_1".to_string(),
		market: "BTC-USDT".to_string(),
		side: Side::Buy,
		price: 50000,
		size: 10,
		timestamp: 1000,
		public_key: "buyer".to_string(),
	};

	let sell_order = OrderCommand {
		order_id: "order_2".to_string(),
		market: "BTC-USDT".to_string(),
		side: Side::Sell,
		price: 49000,
		size: 5,
		timestamp: 1001,
		public_key: "seller".to_string(),
	};

	queue_sender.try_enqueue(buy_order.clone()).unwrap();
	queue_sender.try_enqueue(sell_order.clone()).unwrap();

	// Wait for processing
	std::thread::sleep(std::time::Duration::from_millis(500));

	// Create snapshot
	let snapshot = matching_engine.create_snapshot().unwrap();
	assert!(snapshot.metadata.size_bytes > 0);
	assert!(snapshot.metadata.event_seq > 0);

	// Phase 2: Simulate crash and recovery
	// Drop the engine and event writer
	drop(matching_engine);
	drop(_event_writer);

	// Wait for cleanup
	std::thread::sleep(std::time::Duration::from_millis(200));

	// Phase 3: Start fresh engine and restore from snapshot
	let new_ingress_queue = IngressQueue::new(100);
	let (_new_queue_sender, new_queue_receiver) = new_ingress_queue.split();

	let new_event_buffer = EventBuffer::new(100);
	let (new_event_producer, new_event_consumer) = new_event_buffer.split();

	let new_event_storage = Box::new(MemoryEventStorage::new());
	let _new_event_writer = EventWriter::start(
		new_event_consumer,
		new_event_storage,
		journal.clone(),
		EventWriterConfig {
			batch_size: 10,
			batch_timeout_ms: 100,
			verbose_logging: true,
		},
	);

	let new_engine = MatchingEngine::start(
		engine_config,
		new_queue_receiver,
		new_event_producer,
		journal.clone(),
	);

	// Restore from snapshot
	let restore_result = new_engine.restore_from_snapshot(snapshot.clone());
	assert!(
		restore_result.is_ok(),
		"Failed to restore snapshot: {:?}",
		restore_result.err()
	);

	// Verify restoration by creating another snapshot
	std::thread::sleep(std::time::Duration::from_millis(200));
	let new_snapshot = new_engine.create_snapshot().unwrap();
	assert_eq!(new_snapshot.metadata.event_seq, snapshot.metadata.event_seq);

	drop(new_engine);
	drop(_new_event_writer);
}

#[test]
fn test_event_replay_reconstructs_orderbook() {
	// Phase 1: Create initial engine and events
	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	let ingress_queue = IngressQueue::new(100);
	let (_queue_sender, queue_receiver) = ingress_queue.split();

	let event_buffer = EventBuffer::new(100);
	let (event_producer, event_consumer) = event_buffer.split();

	let event_storage = Box::new(MemoryEventStorage::new());

	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		EventWriterConfig {
			batch_size: 10,
			batch_timeout_ms: 100,
			verbose_logging: true,
		},
	);

	let engine_config = EngineConfig {
		market: "BTC-USDT".to_string(),
		verbose_logging: true,
	};

	let matching_engine = MatchingEngine::start(
		engine_config,
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	// Create test events manually
	let events = vec![
		MatchingEvent::OrderAccepted {
			seq: 1,
			order_id: "order_1".to_string(),
			market: "BTC-USDT".to_string(),
			side: Side::Buy,
			price: 50000,
			size: 10,
			timestamp: 1000,
		},
		MatchingEvent::OrderAccepted {
			seq: 2,
			order_id: "order_2".to_string(),
			market: "BTC-USDT".to_string(),
			side: Side::Sell,
			price: 51000,
			size: 5,
			timestamp: 1001,
		},
	];

	// Replay events
	let replay_result = matching_engine.replay_events(events);
	assert!(
		replay_result.is_ok(),
		"Failed to replay events: {:?}",
		replay_result.err()
	);

	// Wait for processing
	std::thread::sleep(std::time::Duration::from_millis(200));

	// Verify by creating snapshot (should contain replayed orders)
	let snapshot = matching_engine.create_snapshot().unwrap();
	assert!(snapshot.metadata.size_bytes > 0);

	drop(matching_engine);
	drop(_event_writer);
}

#[test]
fn test_maker_order_events_emitted() {
	// This test verifies that maker order events are correctly emitted during matching
	let journal: Box<dyn OrderJournal> = Box::new(MemoryOrderJournal::new());
	let journal = Arc::new(Mutex::new(journal));

	let ingress_queue = IngressQueue::new(100);
	let (queue_sender, queue_receiver) = ingress_queue.split();

	let event_buffer = EventBuffer::new(100);
	let (event_producer, event_consumer) = event_buffer.split();

	let event_storage = Box::new(MemoryEventStorage::new());
	let event_storage_ref = unsafe {
		// This is for testing only - we need to access events after writing
		let ptr = &*event_storage as *const MemoryEventStorage;
		&*ptr
	};

	let _event_writer = EventWriter::start(
		event_consumer,
		event_storage,
		journal.clone(),
		EventWriterConfig {
			batch_size: 5,
			batch_timeout_ms: 50,
			verbose_logging: true,
		},
	);

	let engine_config = EngineConfig {
		market: "BTC-USDT".to_string(),
		verbose_logging: true,
	};

	let _matching_engine = MatchingEngine::start(
		engine_config,
		queue_receiver,
		event_producer,
		journal.clone(),
	);

	// Submit maker order first (limit order on the book)
	let maker_order = OrderCommand {
		order_id: "maker_1".to_string(),
		market: "BTC-USDT".to_string(),
		side: Side::Sell,
		price: 50000,
		size: 10,
		timestamp: 1000,
		public_key: "maker".to_string(),
	};

	queue_sender.try_enqueue(maker_order).unwrap();
	std::thread::sleep(std::time::Duration::from_millis(100));

	// Submit taker order that will match
	let taker_order = OrderCommand {
		order_id: "taker_1".to_string(),
		market: "BTC-USDT".to_string(),
		side: Side::Buy,
		price: 50000,
		size: 5,
		timestamp: 1001,
		public_key: "taker".to_string(),
	};

	queue_sender.try_enqueue(taker_order).unwrap();
	std::thread::sleep(std::time::Duration::from_millis(300));

	// Check that events include MakerOrderPartiallyFilled
	let events = event_storage_ref.replay_from(1).unwrap();
	let has_maker_partial = events
		.iter()
		.any(|e| matches!(e, MatchingEvent::MakerOrderPartiallyFilled { .. }));

	assert!(
		has_maker_partial,
		"Expected MakerOrderPartiallyFilled event but didn't find it"
	);

	drop(_matching_engine);
	drop(_event_writer);
}
