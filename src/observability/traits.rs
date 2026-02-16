use std::time::Duration;

/// Events the observer can record
#[derive(Debug, Clone)]
pub enum ObserverEvent {
    AgentStart {
        provider: String,
        model: String,
    },
    /// A request is about to be sent to an LLM provider.
    ///
    /// This is emitted immediately before a provider call so observers can print
    /// user-facing progress without leaking prompt contents.
    LlmRequest {
        provider: String,
        model: String,
        messages_count: usize,
    },
    /// Result of a single LLM provider call.
    LlmResponse {
        provider: String,
        model: String,
        duration: Duration,
        success: bool,
        error_message: Option<String>,
    },
    AgentEnd {
        duration: Duration,
        tokens_used: Option<u64>,
    },
    /// A tool call is about to be executed.
    ToolCallStart {
        tool: String,
    },
    ToolCall {
        tool: String,
        duration: Duration,
        success: bool,
    },
    /// The agent produced a final answer for the current user message.
    TurnComplete,
    ChannelMessage {
        channel: String,
        direction: String,
    },
    HeartbeatTick,
    Error {
        component: String,
        message: String,
    },
}

/// Numeric metrics
#[derive(Debug, Clone)]
pub enum ObserverMetric {
    RequestLatency(Duration),
    TokensUsed(u64),
    ActiveSessions(u64),
    QueueDepth(u64),
}

/// Core observability trait — implement for any backend
pub trait Observer: Send + Sync + 'static {
    /// Record a discrete event
    fn record_event(&self, event: &ObserverEvent);

    /// Record a numeric metric
    fn record_metric(&self, metric: &ObserverMetric);

    /// Flush any buffered data (no-op for most backends)
    fn flush(&self) {}

    /// Human-readable name of this observer
    fn name(&self) -> &str;

    /// Downcast to `Any` for backend-specific operations
    fn as_any(&self) -> &dyn std::any::Any
    where
        Self: Sized,
    {
        self
    }
}
