use crate::config::Config;
use crate::memory::{self, Memory, MemoryCategory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools;
use anyhow::Result;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

/// Build context preamble by searching memory for relevant entries
async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    let mut context = String::new();

    // Pull relevant memories for this message
    if let Ok(entries) = mem.recall(user_msg, 5).await {
        if !entries.is_empty() {
            context.push_str("[Memory context]\n");
            for entry in &entries {
                let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
            }
            context.push('\n');
        }
    }

    context
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    config: Config,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    // ── Wire up agnostic subsystems ──────────────────────────────
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let _runtime = runtime::create_runtime(&config.runtime)?;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));

    // ── Memory (the brain) ────────────────────────────────────────
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);
    tracing::info!(backend = mem.name(), "Memory initialized");

    // ── Tools (including memory tools) ────────────────────────────
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let _tools = tools::all_tools(&security, mem.clone(), composio_key, &config.browser);

    // ── Resolve provider ─────────────────────────────────────────
    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");

    let model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");

    let provider: Box<dyn Provider> = providers::create_resilient_provider(
        provider_name,
        config.api_key.as_deref(),
        &config.reliability,
    )?;

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    // ── Build system prompt from workspace MD files (OpenClaw framework) ──
    let skills = crate::skills::load_skills(&config.workspace_dir);
    let mut tool_descs: Vec<(&str, &str)> = vec![
        (
            "shell",
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
        ),
        (
            "file_read",
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
        ),
        (
            "file_write",
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
        ),
        (
            "memory_store",
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
        ),
        (
            "memory_recall",
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
        ),
        (
            "memory_forget",
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
        ),
    ];
    if config.browser.enabled {
        tool_descs.push((
            "browser_open",
            "Open approved HTTPS URLs in Brave Browser (allowlist-only, no scraping)",
        ));
    }
    let system_prompt = crate::channels::build_system_prompt(
        &config.workspace_dir,
        model_name,
        &tool_descs,
        &skills,
    );

    // ── Execute ──────────────────────────────────────────────────
    let start = Instant::now();

    if let Some(msg) = message {
        // Auto-save user message to memory
        if config.memory.auto_save {
            let _ = mem
                .store("user_msg", &msg, MemoryCategory::Conversation)
                .await;
        }

        // Inject memory context into user message
        let context = build_context(mem.as_ref(), &msg).await;
        let enriched = if context.is_empty() {
            msg.clone()
        } else {
            format!("{context}{msg}")
        };

        let response = provider
            .chat_with_system(Some(&system_prompt), &enriched, model_name, temperature)
            .await?;
        println!("{response}");

        // Auto-save assistant response to daily log
        if config.memory.auto_save {
            let summary = if response.len() > 100 {
                format!("{}...", &response[..100])
            } else {
                response.clone()
            };
            let _ = mem
                .store("assistant_resp", &summary, MemoryCategory::Daily)
                .await;
        }
    } else {
        println!("🦀 ZeroClaw Interactive Mode");
        println!("Type /quit to exit.\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::channels::CliChannel::new();

        // Spawn listener
        let listen_handle = tokio::spawn(async move {
            let _ = crate::channels::Channel::listen(&cli, tx).await;
        });

        while let Some(msg) = rx.recv().await {
            // Auto-save conversation turns
            if config.memory.auto_save {
                let _ = mem
                    .store("user_msg", &msg.content, MemoryCategory::Conversation)
                    .await;
            }

            // Inject memory context into user message
            let context = build_context(mem.as_ref(), &msg.content).await;
            let enriched = if context.is_empty() {
                msg.content.clone()
            } else {
                format!("{context}{}", msg.content)
            };

            let response = provider
                .chat_with_system(Some(&system_prompt), &enriched, model_name, temperature)
                .await?;
            println!("\n{response}\n");

            if config.memory.auto_save {
                let summary = if response.len() > 100 {
                    format!("{}...", &response[..100])
                } else {
                    response.clone()
                };
                let _ = mem
                    .store("assistant_resp", &summary, MemoryCategory::Daily)
                    .await;
            }
        }

        listen_handle.abort();
    }

    let duration = start.elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        duration,
        tokens_used: None,
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Memory, MemoryCategory, MemoryEntry};
    use crate::providers::Provider;
    use async_trait::async_trait;

    // ── Mock Memory Implementation ──────────────────────────────────
    struct MockMemory {
        entries: Vec<MemoryEntry>,
        should_fail: bool,
    }

    impl MockMemory {
        fn new(entries: Vec<MemoryEntry>) -> Self {
            Self {
                entries,
                should_fail: false,
            }
        }

        fn new_failing() -> Self {
            Self {
                entries: vec![],
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl Memory for MockMemory {
        fn name(&self) -> &str {
            "mock"
        }

        async fn store(
            &self,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall(&self, _query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
            if self.should_fail {
                anyhow::bail!("mock memory recall failed");
            }
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(vec![])
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(0)
        }

        async fn health_check(&self) -> bool {
            true
        }
    }

    // ── Mock Provider Implementation ────────────────────────────────
    struct MockProvider {
        response: String,
        should_fail: bool,
    }

    impl MockProvider {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_string(),
                should_fail: false,
            }
        }

        fn new_failing(error_msg: &str) -> Self {
            Self {
                response: error_msg.to_string(),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            if self.should_fail {
                anyhow::bail!("{}", self.response);
            }
            Ok(self.response.clone())
        }
    }

    // ── Tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_build_context_with_empty_memory() {
        let mem = MockMemory::new(vec![]);
        let context = build_context(&mem, "test query").await;
        assert_eq!(context, "");
    }

    #[tokio::test]
    async fn test_build_context_with_memories() {
        let entries = vec![
            MemoryEntry {
                id: "1".to_string(),
                key: "preference".to_string(),
                content: "User prefers concise responses".to_string(),
                category: MemoryCategory::Core,
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                session_id: None,
                score: Some(0.95),
            },
            MemoryEntry {
                id: "2".to_string(),
                key: "context".to_string(),
                content: "Working on Rust project".to_string(),
                category: MemoryCategory::Conversation,
                timestamp: "2024-01-01T00:01:00Z".to_string(),
                session_id: None,
                score: Some(0.85),
            },
        ];

        let mem = MockMemory::new(entries);
        let context = build_context(&mem, "test query").await;

        assert!(context.contains("[Memory context]"));
        assert!(context.contains("preference: User prefers concise responses"));
        assert!(context.contains("context: Working on Rust project"));
    }

    #[tokio::test]
    async fn test_build_context_memory_error() {
        let mem = MockMemory::new_failing();
        let context = build_context(&mem, "test query").await;
        // Should return empty string on error, not panic
        assert_eq!(context, "");
    }

    #[tokio::test]
    async fn test_build_context_respects_limit() {
        let entries = vec![
            MemoryEntry {
                id: "1".to_string(),
                key: "key1".to_string(),
                content: "content1".to_string(),
                category: MemoryCategory::Core,
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                session_id: None,
                score: Some(0.9),
            },
            MemoryEntry {
                id: "2".to_string(),
                key: "key2".to_string(),
                content: "content2".to_string(),
                category: MemoryCategory::Core,
                timestamp: "2024-01-01T00:01:00Z".to_string(),
                session_id: None,
                score: Some(0.8),
            },
            MemoryEntry {
                id: "3".to_string(),
                key: "key3".to_string(),
                content: "content3".to_string(),
                category: MemoryCategory::Core,
                timestamp: "2024-01-01T00:02:00Z".to_string(),
                session_id: None,
                score: Some(0.7),
            },
        ];

        let mem = MockMemory::new(entries);
        let context = build_context(&mem, "test query").await;

        // build_context calls recall with limit=5, but we only have 3 entries
        // All 3 should be included
        assert!(context.contains("key1"));
        assert!(context.contains("key2"));
        assert!(context.contains("key3"));
    }

    #[tokio::test]
    async fn test_build_context_formats_correctly() {
        let entries = vec![MemoryEntry {
            id: "1".to_string(),
            key: "test_key".to_string(),
            content: "test content".to_string(),
            category: MemoryCategory::Core,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            session_id: None,
            score: Some(0.95),
        }];

        let mem = MockMemory::new(entries);
        let context = build_context(&mem, "test query").await;

        // Verify format: "[Memory context]\n- key: content\n\n"
        assert!(context.starts_with("[Memory context]\n"));
        assert!(context.contains("- test_key: test content\n"));
        assert!(context.ends_with('\n'));
    }

    #[tokio::test]
    async fn test_provider_error_handling() {
        let provider = MockProvider::new_failing("API rate limit exceeded");

        let result = provider
            .chat_with_system(Some("system"), "user message", "test-model", 0.7)
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("API rate limit exceeded"));
    }

    #[tokio::test]
    async fn test_provider_success() {
        let provider = MockProvider::new("Hello from mock provider");

        let result = provider
            .chat_with_system(Some("system"), "user message", "test-model", 0.7)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello from mock provider");
    }
}
