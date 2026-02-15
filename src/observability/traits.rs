//! Observer trait — implement for any observability backend.
//!
//! This module defines the core abstraction for observability and monitoring. Implement the
//! `Observer` trait to send metrics and events to any backend (Prometheus, OpenTelemetry, logs, etc.).

use std::time::Duration;

/// Events the observer can record.
///
/// This enum represents discrete events that occur during agent operation,
/// such as agent starts/stops, tool calls, and errors.
#[derive(Debug, Clone)]
pub enum ObserverEvent {
    /// Agent started processing a request
    AgentStart {
        /// Provider name (e.g., "openai", "anthropic")
        provider: String,
        /// Model identifier
        model: String,
    },
    /// Agent finished processing a request
    AgentEnd {
        /// Total processing duration
        duration: Duration,
        /// Number of tokens used (if available)
        tokens_used: Option<u64>,
    },
    /// Tool was called by the agent
    ToolCall {
        /// Tool name
        tool: String,
        /// Tool execution duration
        duration: Duration,
        /// Whether the tool succeeded
        success: bool,
    },
    /// Message sent or received on a channel
    ChannelMessage {
        /// Channel name
        channel: String,
        /// Direction ("inbound" or "outbound")
        direction: String,
    },
    /// Heartbeat tick occurred
    HeartbeatTick,
    /// Error occurred in a component
    Error {
        /// Component where the error occurred
        component: String,
        /// Error message
        message: String,
    },
}

/// Numeric metrics.
///
/// This enum represents numeric measurements that can be tracked over time,
/// such as latencies, counts, and gauges.
#[derive(Debug, Clone)]
pub enum ObserverMetric {
    /// Request latency measurement
    RequestLatency(Duration),
    /// Number of tokens used
    TokensUsed(u64),
    /// Number of active sessions
    ActiveSessions(u64),
    /// Queue depth measurement
    QueueDepth(u64),
}

/// Core observability trait — implement for any backend.
///
/// This trait abstracts over different observability backends, allowing `ZeroClaw` to
/// send metrics and events to any monitoring system. Implementations handle backend-specific
/// formatting, batching, and transmission.
///
/// # Implementation Guide
///
/// 1. Implement `record_event()` to handle discrete events
/// 2. Implement `record_metric()` to handle numeric measurements
/// 3. Optionally override `flush()` if your backend buffers data
/// 4. Implement `name()` to identify your observer
/// 5. Register your observer in the observability configuration
///
/// # Example
///
/// ```ignore
/// use zeroclaw::observability::traits::{Observer, ObserverEvent, ObserverMetric};
///
/// pub struct PrometheusObserver {
///     // Prometheus client
/// }
///
/// impl Observer for PrometheusObserver {
///     fn record_event(&self, event: &ObserverEvent) {
///         // Convert event to Prometheus metric
///     }
///
///     fn record_metric(&self, metric: &ObserverMetric) {
///         // Record metric in Prometheus
///     }
///
///     fn name(&self) -> &str {
///         "prometheus"
///     }
/// }
/// ```
pub trait Observer: Send + Sync {
    /// Record a discrete event.
    ///
    /// # Parameters
    ///
    /// - `event`: The event to record
    fn record_event(&self, event: &ObserverEvent);

    /// Record a numeric metric.
    ///
    /// # Parameters
    ///
    /// - `metric`: The metric to record
    fn record_metric(&self, metric: &ObserverMetric);

    /// Flush any buffered data to the backend.
    ///
    /// The default implementation is a no-op. Override this if your backend
    /// buffers data and needs periodic flushing.
    fn flush(&self) {}

    /// Human-readable name of this observer (e.g., "prometheus", "log").
    ///
    /// This name is used for logging and identification.
    fn name(&self) -> &str;
}
