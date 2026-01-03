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

//! OpenTelemetry tracer initialization for Gateway service
//!
//! This module provides OpenTelemetry tracer setup with support for both
//! OTLP export (for external observability backends) and local tracing.
//!
//! # Configuration
//!
//! The tracer can be configured via environment variables:
//!
//! - `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP endpoint URL (e.g., `http://localhost:4317`)
//!   - If set, traces are exported to the OTLP endpoint
//!   - If not set, traces are only available locally via the tracing layer
//!
//! # Trace Context Propagation
//!
//! The module configures `TraceContextPropagator` as the global text map propagator,
//! enabling W3C Trace Context (`traceparent`, `tracestate`) propagation across
//! HTTP and gRPC boundaries.
//!
//! # Sampling
//!
//! The tracer uses `ParentBased(AlwaysOn)` sampling:
//! - If a parent trace context exists (from upstream), respect its sampling decision
//! - If no parent exists, always sample (create spans)

use anyhow::Result;
use opentelemetry::{KeyValue, global, trace::TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
	propagation::TraceContextPropagator, resource::Resource, trace as sdktrace,
};

/// Service name for OpenTelemetry resource identification
const SERVICE_NAME: &str = "anvil-gateway";

/// Initialize OpenTelemetry tracer with W3C Trace Context propagation
///
/// This function sets up OpenTelemetry tracing for the gateway service:
///
/// 1. **Propagator**: Configures `TraceContextPropagator` as the global text map
///    propagator for W3C Trace Context (`traceparent`, `tracestate`) propagation.
///
/// 2. **Resource**: Creates a resource with service name `anvil-gateway` for
///    identifying traces from this service.
///
/// 3. **Sampler**: Uses `ParentBased(AlwaysOn)` sampling to respect upstream
///    sampling decisions while always sampling new traces.
///
/// 4. **Exporter**: If `OTEL_EXPORTER_OTLP_ENDPOINT` is set, configures OTLP export
///    to the specified endpoint. Otherwise, uses a local tracer provider.
///
/// # Environment Variables
///
/// - `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP endpoint URL (optional)
///
/// # Returns
///
/// Returns `Ok(Some(Tracer))` if tracing is successfully initialized, or
/// `Ok(None)` if initialization failed (non-fatal, logging continues without OTel).
pub fn init_tracer() -> Result<Option<sdktrace::Tracer>> {
	// Set W3C Trace Context propagator as the global text map propagator
	// This enables trace context propagation via HTTP headers (traceparent, tracestate)
	// and gRPC metadata across service boundaries
	global::set_text_map_propagator(TraceContextPropagator::new());

	// Create OpenTelemetry resource with service name for trace identification
	let resource = Resource::new(vec![KeyValue::new("service.name", SERVICE_NAME)]);

	// Create trace configuration with parent-based sampling
	// ParentBased(AlwaysOn): respect upstream sampling decisions, always sample new traces
	let make_config = || {
		sdktrace::Config::default()
			.with_sampler(sdktrace::Sampler::ParentBased(Box::new(
				sdktrace::Sampler::AlwaysOn,
			)))
			.with_resource(resource.clone())
	};

	// Check if OTLP endpoint is configured for external trace export
	let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

	let tracer = if let Some(endpoint) = otlp_endpoint {
		// OTLP export mode: export traces to external observability backend
		let exporter = opentelemetry_otlp::new_exporter()
			.tonic()
			.with_endpoint(endpoint);
		opentelemetry_otlp::new_pipeline()
			.tracing()
			.with_exporter(exporter)
			.with_trace_config(make_config())
			.install_batch(opentelemetry_sdk::runtime::Tokio)?
	} else {
		// Local mode: traces are only available via tracing layer (no external export)
		let provider = sdktrace::TracerProvider::builder()
			.with_config(make_config())
			.build();
		let tracer = provider.tracer(SERVICE_NAME);
		global::set_tracer_provider(provider);
		tracer
	};

	Ok(Some(tracer))
}
