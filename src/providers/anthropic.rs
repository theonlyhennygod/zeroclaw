use crate::auth::anthropic_token::{detect_auth_kind, AnthropicAuthKind};
use crate::auth::AuthService;
use crate::providers::traits::Provider;
use crate::providers::ProviderRuntimeOptions;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct AnthropicProvider {
    api_key: Option<String>,
    auth_kind: AnthropicAuthKind,
    explicit_kind_override: Option<AnthropicAuthKind>,
    auth_service: Option<AuthService>,
    auth_profile_override: Option<String>,
    client: Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<Message>,
    temperature: f64,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: String,
}

impl AnthropicProvider {
    pub fn new(api_key: Option<&str>) -> Self {
        Self {
            api_key: api_key.map(ToString::to_string),
            auth_kind: api_key
                .map_or(AnthropicAuthKind::ApiKey, |token| detect_auth_kind(token, None)),
            explicit_kind_override: None,
            auth_service: None,
            auth_profile_override: None,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub fn new_with_options(
        api_key: Option<&str>,
        options: &ProviderRuntimeOptions,
    ) -> Self {
        let explicit_kind_override = std::env::var("ANTHROPIC_AUTH_KIND")
            .ok()
            .as_deref()
            .and_then(AnthropicAuthKind::from_metadata_value);

        let mut resolved = api_key
            .map(|value| (value.to_string(), None))
            .or_else(|| std::env::var("ANTHROPIC_AUTH_TOKEN").ok().map(|v| (v, Some(AnthropicAuthKind::Authorization))))
            .or_else(|| std::env::var("ANTHROPIC_OAUTH_TOKEN").ok().map(|v| (v, Some(AnthropicAuthKind::Authorization))))
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().map(|v| (v, Some(AnthropicAuthKind::ApiKey))));

        let (token, kind_from_source) = match resolved.take() {
            Some((token, kind)) if !token.trim().is_empty() => (Some(token), kind),
            _ => (None, None),
        };

        let auth_kind = token
            .as_deref()
            .map_or(AnthropicAuthKind::ApiKey, |token| {
                explicit_kind_override
                    .or(kind_from_source)
                    .unwrap_or_else(|| detect_auth_kind(token, None))
            });

        let state_dir = options
            .zeroclaw_dir
            .clone()
            .unwrap_or_else(default_zeroclaw_dir);
        let auth_service = Some(AuthService::new(&state_dir, options.secrets_encrypt));

        Self {
            api_key: token,
            auth_kind,
            explicit_kind_override,
            auth_service,
            auth_profile_override: options.auth_profile_override.clone(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn resolve_auth(&self) -> anyhow::Result<(String, AnthropicAuthKind)> {
        if let Some(token) = self.api_key.clone() {
            return Ok((token, self.auth_kind));
        }

        if let Some(auth_service) = &self.auth_service {
            if let Some(profile) =
                auth_service.get_profile("anthropic", self.auth_profile_override.as_deref())?
            {
                let token = match profile.kind {
                    crate::auth::profiles::AuthProfileKind::Token => profile.token,
                    crate::auth::profiles::AuthProfileKind::OAuth => {
                        profile.token_set.map(|token_set| token_set.access_token)
                    }
                };

                if let Some(token) = token {
                    if !token.trim().is_empty() {
                        let kind = profile
                            .metadata
                            .get("auth_kind")
                            .and_then(|value| AnthropicAuthKind::from_metadata_value(value))
                            .or(self.explicit_kind_override)
                            .unwrap_or_else(|| detect_auth_kind(&token, None));
                        return Ok((token, kind));
                    }
                }
            }
        }

        anyhow::bail!(
            "Anthropic auth not configured. Set ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN or run `zeroclaw auth paste-token --provider anthropic`."
        )
    }
}

fn default_zeroclaw_dir() -> PathBuf {
    directories::UserDirs::new().map_or_else(
        || PathBuf::from(".zeroclaw"),
        |dirs| dirs.home_dir().join(".zeroclaw"),
    )
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let (token, auth_kind) = self.resolve_auth()?;

        let request = ChatRequest {
            model: model.to_string(),
            max_tokens: 4096,
            system: system_prompt.map(ToString::to_string),
            messages: vec![Message {
                role: "user".to_string(),
                content: message.to_string(),
            }],
            temperature,
        };

        let mut builder = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");

        builder = match auth_kind {
            AnthropicAuthKind::ApiKey => builder.header("x-api-key", token),
            AnthropicAuthKind::Authorization => {
                builder.header("Authorization", format!("Bearer {token}"))
            }
        };

        let response = builder.json(&request).send().await?;

        if !response.status().is_success() {
            return Err(super::api_error("Anthropic", response).await);
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .content
            .into_iter()
            .next()
            .map(|c| c.text)
            .ok_or_else(|| anyhow::anyhow!("No response from Anthropic"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_with_key() {
        let p = AnthropicProvider::new(Some("sk-ant-test123"));
        assert!(p.api_key.is_some());
        assert_eq!(p.api_key.as_deref(), Some("sk-ant-test123"));
    }

    #[test]
    fn creates_without_key() {
        let p = AnthropicProvider::new(None);
        assert!(p.api_key.is_none());
    }

    #[test]
    fn creates_with_empty_key() {
        let p = AnthropicProvider::new(Some(""));
        assert!(p.api_key.is_some());
        assert_eq!(p.api_key.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn chat_fails_without_key() {
        let p = AnthropicProvider::new(None);
        let result = p
            .chat_with_system(None, "hello", "claude-3-opus", 0.7)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("auth not configured"), "Expected auth error, got: {err}");
    }

    #[tokio::test]
    async fn chat_with_system_fails_without_key() {
        let p = AnthropicProvider::new(None);
        let result = p
            .chat_with_system(Some("You are ZeroClaw"), "hello", "claude-3-opus", 0.7)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn chat_request_serializes_without_system() {
        let req = ChatRequest {
            model: "claude-3-opus".to_string(),
            max_tokens: 4096,
            system: None,
            messages: vec![Message {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            temperature: 0.7,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(
            !json.contains("system"),
            "system field should be skipped when None"
        );
        assert!(json.contains("claude-3-opus"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn chat_request_serializes_with_system() {
        let req = ChatRequest {
            model: "claude-3-opus".to_string(),
            max_tokens: 4096,
            system: Some("You are ZeroClaw".to_string()),
            messages: vec![Message {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            temperature: 0.7,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"system\":\"You are ZeroClaw\""));
    }

    #[test]
    fn chat_response_deserializes() {
        let json = r#"{"content":[{"type":"text","text":"Hello there!"}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 1);
        assert_eq!(resp.content[0].text, "Hello there!");
    }

    #[test]
    fn chat_response_empty_content() {
        let json = r#"{"content":[]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.content.is_empty());
    }

    #[test]
    fn chat_response_multiple_blocks() {
        let json =
            r#"{"content":[{"type":"text","text":"First"},{"type":"text","text":"Second"}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 2);
        assert_eq!(resp.content[0].text, "First");
        assert_eq!(resp.content[1].text, "Second");
    }

    #[test]
    fn temperature_range_serializes() {
        for temp in [0.0, 0.5, 1.0, 2.0] {
            let req = ChatRequest {
                model: "claude-3-opus".to_string(),
                max_tokens: 4096,
                system: None,
                messages: vec![],
                temperature: temp,
            };
            let json = serde_json::to_string(&req).unwrap();
            assert!(json.contains(&format!("{temp}")));
        }
    }

    #[test]
    fn detects_auth_from_jwt_shape() {
        let kind = detect_auth_kind("a.b.c", None);
        assert_eq!(kind, AnthropicAuthKind::Authorization);
    }
}
