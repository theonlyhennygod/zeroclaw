//! Tunnel module — expose the gateway through any tunneling service.
//!
//! This module provides the `Tunnel` trait and implementations for popular tunnel providers
//! (Cloudflare, Tailscale, ngrok, custom). Tunnels allow the gateway to be accessed from
//! the internet without exposing ports directly.

mod cloudflare;
mod custom;
mod ngrok;
mod none;
mod tailscale;

pub use cloudflare::CloudflareTunnel;
pub use custom::CustomTunnel;
pub use ngrok::NgrokTunnel;
#[allow(unused_imports)]
pub use none::NoneTunnel;
pub use tailscale::TailscaleTunnel;

use crate::config::schema::{TailscaleTunnelConfig, TunnelConfig};
use anyhow::{bail, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Tunnel trait ─────────────────────────────────────────────────

/// Tunnel trait — implement for any tunnel provider.
///
/// This trait abstracts over different tunnel providers, allowing `ZeroClaw` to expose
/// its gateway through any tunneling service. Implementations wrap external tunnel
/// binaries (cloudflared, tailscale, ngrok, etc.) or custom commands.
///
/// # Implementation Guide
///
/// 1. Implement `name()` to identify your tunnel provider
/// 2. Implement `start()` to spawn the tunnel process and extract the public URL
/// 3. Implement `stop()` to gracefully terminate the tunnel
/// 4. Implement `health_check()` to verify the tunnel is still running
/// 5. Implement `public_url()` to return the current public URL
/// 6. Register your tunnel in the tunnel configuration
///
/// # Lifecycle
///
/// The gateway calls `start()` after binding its local port and `stop()` on shutdown.
/// The tunnel should remain running until `stop()` is called.
///
/// # Example
///
/// See the built-in implementations in this module (`CloudflareTunnel`, `TailscaleTunnel`,
/// `NgrokTunnel`, `CustomTunnel`) for reference.
#[async_trait::async_trait]
pub trait Tunnel: Send + Sync {
    /// Human-readable provider name (e.g., "cloudflare", "tailscale", "ngrok").
    ///
    /// This name is used for logging and identification.
    fn name(&self) -> &str;

    /// Start the tunnel, exposing the local server externally.
    ///
    /// This method should spawn the tunnel process, wait for it to establish
    /// a connection, and extract the public URL from its output.
    ///
    /// # Parameters
    ///
    /// - `local_host`: The local host to expose (e.g., "127.0.0.1")
    /// - `local_port`: The local port to expose (e.g., 8080)
    ///
    /// # Returns
    ///
    /// The public URL where the tunnel is accessible (e.g., <https://abc123.cloudflare.com>).
    ///
    /// # Errors
    ///
    /// Returns an error if the tunnel process fails to start or the public URL
    /// cannot be extracted.
    async fn start(&self, local_host: &str, local_port: u16) -> Result<String>;

    /// Stop the tunnel process gracefully.
    ///
    /// This method should terminate the tunnel process and clean up any resources.
    ///
    /// # Errors
    ///
    /// Returns an error if the tunnel cannot be stopped cleanly.
    async fn stop(&self) -> Result<()>;

    /// Check if the tunnel is still alive and healthy.
    ///
    /// # Returns
    ///
    /// `true` if the tunnel is running and healthy, `false` otherwise.
    async fn health_check(&self) -> bool;

    /// Return the public URL if the tunnel is running.
    ///
    /// # Returns
    ///
    /// `Some(url)` if the tunnel is active, `None` if not started or stopped.
    fn public_url(&self) -> Option<String>;
}

// ── Shared child-process handle ──────────────────────────────────

/// Wraps a spawned tunnel child process so implementations can share it.
pub(crate) struct TunnelProcess {
    pub child: tokio::process::Child,
    pub public_url: String,
}

pub(crate) type SharedProcess = Arc<Mutex<Option<TunnelProcess>>>;

pub(crate) fn new_shared_process() -> SharedProcess {
    Arc::new(Mutex::new(None))
}

/// Kill a shared tunnel process if running.
pub(crate) async fn kill_shared(proc: &SharedProcess) -> Result<()> {
    let mut guard = proc.lock().await;
    if let Some(ref mut tp) = *guard {
        tp.child.kill().await.ok();
        tp.child.wait().await.ok();
    }
    *guard = None;
    Ok(())
}

// ── Factory ──────────────────────────────────────────────────────

/// Create a tunnel from config. Returns `None` for provider "none".
pub fn create_tunnel(config: &TunnelConfig) -> Result<Option<Box<dyn Tunnel>>> {
    match config.provider.as_str() {
        "none" | "" => Ok(None),

        "cloudflare" => {
            let cf = config
                .cloudflare
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("tunnel.provider = \"cloudflare\" but [tunnel.cloudflare] section is missing"))?;
            Ok(Some(Box::new(CloudflareTunnel::new(cf.token.clone()))))
        }

        "tailscale" => {
            let ts = config.tailscale.as_ref().unwrap_or(&TailscaleTunnelConfig {
                funnel: false,
                hostname: None,
            });
            Ok(Some(Box::new(TailscaleTunnel::new(
                ts.funnel,
                ts.hostname.clone(),
            ))))
        }

        "ngrok" => {
            let ng = config
                .ngrok
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("tunnel.provider = \"ngrok\" but [tunnel.ngrok] section is missing"))?;
            Ok(Some(Box::new(NgrokTunnel::new(
                ng.auth_token.clone(),
                ng.domain.clone(),
            ))))
        }

        "custom" => {
            let cu = config
                .custom
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("tunnel.provider = \"custom\" but [tunnel.custom] section is missing"))?;
            Ok(Some(Box::new(CustomTunnel::new(
                cu.start_command.clone(),
                cu.health_url.clone(),
                cu.url_pattern.clone(),
            ))))
        }

        other => bail!("Unknown tunnel provider: \"{other}\". Valid: none, cloudflare, tailscale, ngrok, custom"),
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, TunnelConfig,
    };

    /// Helper: assert `create_tunnel` returns an error containing `needle`.
    fn assert_tunnel_err(cfg: &TunnelConfig, needle: &str) {
        match create_tunnel(cfg) {
            Err(e) => assert!(
                e.to_string().contains(needle),
                "Expected error containing \"{needle}\", got: {e}"
            ),
            Ok(_) => panic!("Expected error containing \"{needle}\", but got Ok"),
        }
    }

    #[test]
    fn factory_none_returns_none() {
        let cfg = TunnelConfig::default();
        let t = create_tunnel(&cfg).unwrap();
        assert!(t.is_none());
    }

    #[test]
    fn factory_empty_string_returns_none() {
        let cfg = TunnelConfig {
            provider: String::new(),
            ..TunnelConfig::default()
        };
        let t = create_tunnel(&cfg).unwrap();
        assert!(t.is_none());
    }

    #[test]
    fn factory_unknown_provider_errors() {
        let cfg = TunnelConfig {
            provider: "wireguard".into(),
            ..TunnelConfig::default()
        };
        assert_tunnel_err(&cfg, "Unknown tunnel provider");
    }

    #[test]
    fn factory_cloudflare_missing_config_errors() {
        let cfg = TunnelConfig {
            provider: "cloudflare".into(),
            ..TunnelConfig::default()
        };
        assert_tunnel_err(&cfg, "[tunnel.cloudflare]");
    }

    #[test]
    fn factory_cloudflare_with_config_ok() {
        let cfg = TunnelConfig {
            provider: "cloudflare".into(),
            cloudflare: Some(CloudflareTunnelConfig {
                token: "test-token".into(),
            }),
            ..TunnelConfig::default()
        };
        let t = create_tunnel(&cfg).unwrap();
        assert!(t.is_some());
        assert_eq!(t.unwrap().name(), "cloudflare");
    }

    #[test]
    fn factory_tailscale_defaults_ok() {
        let cfg = TunnelConfig {
            provider: "tailscale".into(),
            ..TunnelConfig::default()
        };
        let t = create_tunnel(&cfg).unwrap();
        assert!(t.is_some());
        assert_eq!(t.unwrap().name(), "tailscale");
    }

    #[test]
    fn factory_ngrok_missing_config_errors() {
        let cfg = TunnelConfig {
            provider: "ngrok".into(),
            ..TunnelConfig::default()
        };
        assert_tunnel_err(&cfg, "[tunnel.ngrok]");
    }

    #[test]
    fn factory_ngrok_with_config_ok() {
        let cfg = TunnelConfig {
            provider: "ngrok".into(),
            ngrok: Some(NgrokTunnelConfig {
                auth_token: "tok".into(),
                domain: None,
            }),
            ..TunnelConfig::default()
        };
        let t = create_tunnel(&cfg).unwrap();
        assert!(t.is_some());
        assert_eq!(t.unwrap().name(), "ngrok");
    }

    #[test]
    fn factory_custom_missing_config_errors() {
        let cfg = TunnelConfig {
            provider: "custom".into(),
            ..TunnelConfig::default()
        };
        assert_tunnel_err(&cfg, "[tunnel.custom]");
    }

    #[test]
    fn factory_custom_with_config_ok() {
        let cfg = TunnelConfig {
            provider: "custom".into(),
            custom: Some(CustomTunnelConfig {
                start_command: "echo tunnel".into(),
                health_url: None,
                url_pattern: None,
            }),
            ..TunnelConfig::default()
        };
        let t = create_tunnel(&cfg).unwrap();
        assert!(t.is_some());
        assert_eq!(t.unwrap().name(), "custom");
    }

    #[test]
    fn none_tunnel_name() {
        let t = NoneTunnel;
        assert_eq!(t.name(), "none");
    }

    #[test]
    fn none_tunnel_public_url_is_none() {
        let t = NoneTunnel;
        assert!(t.public_url().is_none());
    }

    #[tokio::test]
    async fn none_tunnel_health_always_true() {
        let t = NoneTunnel;
        assert!(t.health_check().await);
    }

    #[tokio::test]
    async fn none_tunnel_start_returns_local() {
        let t = NoneTunnel;
        let url = t.start("127.0.0.1", 8080).await.unwrap();
        assert_eq!(url, "http://127.0.0.1:8080");
    }

    #[test]
    fn cloudflare_tunnel_name() {
        let t = CloudflareTunnel::new("tok".into());
        assert_eq!(t.name(), "cloudflare");
        assert!(t.public_url().is_none());
    }

    #[test]
    fn tailscale_tunnel_name() {
        let t = TailscaleTunnel::new(false, None);
        assert_eq!(t.name(), "tailscale");
        assert!(t.public_url().is_none());
    }

    #[test]
    fn tailscale_funnel_mode() {
        let t = TailscaleTunnel::new(true, Some("myhost".into()));
        assert_eq!(t.name(), "tailscale");
    }

    #[test]
    fn ngrok_tunnel_name() {
        let t = NgrokTunnel::new("tok".into(), None);
        assert_eq!(t.name(), "ngrok");
        assert!(t.public_url().is_none());
    }

    #[test]
    fn ngrok_with_domain() {
        let t = NgrokTunnel::new("tok".into(), Some("my.ngrok.io".into()));
        assert_eq!(t.name(), "ngrok");
    }

    #[test]
    fn custom_tunnel_name() {
        let t = CustomTunnel::new("echo hi".into(), None, None);
        assert_eq!(t.name(), "custom");
        assert!(t.public_url().is_none());
    }
}
