use crate::config::Config;
use crate::health::HealthSnapshot;
use crate::integrations::{self, IntegrationStatus};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CanvasSnapshot {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub workspace_dir: String,
    pub gateway: CanvasGateway,
    pub lanes: Vec<CanvasLane>,
    pub integrations: CanvasIntegrationSummary,
    pub tools: CanvasToolSummary,
    pub health: HealthSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasGateway {
    pub host: String,
    pub port: u16,
    pub paired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasLane {
    pub id: &'static str,
    pub label: &'static str,
    pub nodes: Vec<CanvasNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasNode {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasIntegrationSummary {
    pub active: usize,
    pub available: usize,
    pub coming_soon: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasToolSummary {
    pub total: usize,
}

pub fn build_snapshot(
    config: &Config,
    model: &str,
    temperature: f64,
    memory_backend: &str,
    paired: bool,
    tool_count: usize,
    health: HealthSnapshot,
) -> CanvasSnapshot {
    let channels: Vec<CanvasNode> = config
        .channels_config
        .channels()
        .into_iter()
        .map(|(channel, present)| CanvasNode {
            id: format!("channel-{}", channel.name()),
            title: channel.name().to_string(),
            detail: if present {
                "Configured and ready".to_string()
            } else {
                "Not configured".to_string()
            },
            status: if present { "active" } else { "idle" },
        })
        .collect();

    let provider = config.default_provider.as_deref().unwrap_or("unconfigured");
    let core = vec![
        CanvasNode {
            id: "agent-core".to_string(),
            title: "Agent Core".to_string(),
            detail: format!("{provider} -> {model} @ {temperature:.1}"),
            status: if config.api_key.is_some() {
                "active"
            } else {
                "idle"
            },
        },
        CanvasNode {
            id: "memory-core".to_string(),
            title: "Memory".to_string(),
            detail: memory_backend.to_string(),
            status: "active",
        },
        CanvasNode {
            id: "pairing-core".to_string(),
            title: "Pairing".to_string(),
            detail: if paired {
                "Trusted dashboard session".to_string()
            } else {
                "Awaiting pairing".to_string()
            },
            status: if paired { "active" } else { "warn" },
        },
    ];

    let integrations_summary = summarize_integrations(config);
    let automation = vec![
        CanvasNode {
            id: "gateway-surface".to_string(),
            title: "Gateway".to_string(),
            detail: format!("{}:{}", config.gateway.host, config.gateway.port),
            status: "active",
        },
        CanvasNode {
            id: "integrations-surface".to_string(),
            title: "Integrations".to_string(),
            detail: format!(
                "{} active / {} available / {} planned",
                integrations_summary.active,
                integrations_summary.available,
                integrations_summary.coming_soon
            ),
            status: if integrations_summary.active > 0 {
                "active"
            } else {
                "idle"
            },
        },
        CanvasNode {
            id: "tools-surface".to_string(),
            title: "Tool Surface".to_string(),
            detail: format!("{tool_count} registered tools"),
            status: if tool_count > 0 { "active" } else { "warn" },
        },
    ];

    CanvasSnapshot {
        title: "ZeroClaw Canvas",
        subtitle: "Live visual workspace for gateway, agent, tools, and channels",
        workspace_dir: config.workspace_dir.display().to_string(),
        gateway: CanvasGateway {
            host: config.gateway.host.clone(),
            port: config.gateway.port,
            paired,
        },
        lanes: vec![
            CanvasLane {
                id: "inputs",
                label: "Inputs",
                nodes: channels,
            },
            CanvasLane {
                id: "core",
                label: "Core",
                nodes: core,
            },
            CanvasLane {
                id: "automation",
                label: "Automation",
                nodes: automation,
            },
        ],
        integrations: integrations_summary,
        tools: CanvasToolSummary { total: tool_count },
        health,
    }
}

fn summarize_integrations(config: &Config) -> CanvasIntegrationSummary {
    let mut summary = CanvasIntegrationSummary {
        active: 0,
        available: 0,
        coming_soon: 0,
    };

    for entry in integrations::registry::all_integrations() {
        match (entry.status_fn)(config) {
            IntegrationStatus::Active => summary.active += 1,
            IntegrationStatus::Available => summary.available += 1,
            IntegrationStatus::ComingSoon => summary.coming_soon += 1,
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{GatewayConfig, TelegramConfig};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn test_health() -> HealthSnapshot {
        HealthSnapshot {
            pid: 42,
            updated_at: "2026-03-11T00:00:00Z".to_string(),
            uptime_seconds: 64,
            components: BTreeMap::from([(
                "gateway".to_string(),
                crate::health::ComponentHealth {
                    status: "ok".to_string(),
                    updated_at: "2026-03-11T00:00:00Z".to_string(),
                    last_ok: Some("2026-03-11T00:00:00Z".to_string()),
                    last_error: None,
                    restart_count: 0,
                },
            )]),
        }
    }

    #[test]
    fn build_snapshot_summarizes_channels_and_integrations() {
        let mut config = Config::default();
        config.workspace_dir = PathBuf::from("/tmp/zeroclaw-workspace");
        config.default_provider = Some("openai".to_string());
        config.gateway = GatewayConfig::default();
        config.channels_config.telegram = Some(TelegramConfig {
            bot_token: "token".to_string(),
            allowed_users: vec!["zeroclaw_user".to_string()],
            stream_mode: crate::config::schema::StreamMode::default(),
            draft_update_interval_ms: 1000,
            interrupt_on_new_message: false,
            mention_only: false,
        });

        let snapshot = build_snapshot(
            &config,
            "gpt-4.1-mini",
            0.7,
            "sqlite",
            true,
            12,
            test_health(),
        );

        assert_eq!(snapshot.title, "ZeroClaw Canvas");
        assert_eq!(snapshot.workspace_dir, "/tmp/zeroclaw-workspace");
        assert_eq!(snapshot.gateway.port, config.gateway.port);
        assert!(snapshot.lanes.iter().any(|lane| lane.label == "Inputs"));
        assert!(snapshot
            .lanes
            .iter()
            .flat_map(|lane| lane.nodes.iter())
            .any(|node| node.title == "Telegram" && node.status == "active"));
        assert_eq!(snapshot.tools.total, 12);
        assert!(snapshot.integrations.available > 0);
        assert!(snapshot.integrations.coming_soon > 0);
    }
}
