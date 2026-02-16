use crate::providers::{ChatMessage, Provider};
use crate::web::session::SessionManager;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    Message {
        content: String,
        session_id: String,
    },
    NewSession,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    Message {
        content: String,
        session_id: String,
    },
    SessionCreated {
        session_id: String,
    },
    Typing,
    Error {
        content: String,
    },
}

impl ServerMessage {
    fn to_text(&self) -> Message {
        Message::Text(serde_json::to_string(self).unwrap_or_else(|_| "{}".into()))
    }
}

pub async fn handle_ws(
    socket: WebSocket,
    sessions: Arc<SessionManager>,
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    system_prompt: String,
) {
    let (mut sender, mut receiver) = socket.split();

    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = sender
                    .send(
                        ServerMessage::Error {
                            content: format!("Invalid message format: {e}"),
                        }
                        .to_text(),
                    )
                    .await;
                continue;
            }
        };

        match client_msg {
            ClientMessage::NewSession => {
                match sessions.create_session().await {
                    Ok(session_id) => {
                        // Add system prompt to the new session
                        let _ = sessions
                            .add_message(
                                &session_id,
                                ChatMessage::system(system_prompt.clone()),
                            )
                            .await;
                        let _ = sender
                            .send(ServerMessage::SessionCreated { session_id }.to_text())
                            .await;
                    }
                    Err(e) => {
                        let _ = sender
                            .send(ServerMessage::Error { content: e }.to_text())
                            .await;
                    }
                }
            }

            ClientMessage::Message {
                content,
                session_id,
            } => {
                // Check session exists; if not, auto-create
                if !sessions.session_exists(&session_id).await {
                    match sessions.create_session().await {
                        Ok(new_id) => {
                            // We can't use the requested ID, but we created a new one
                            // In practice, the client should use NewSession first
                            if new_id != session_id {
                                let _ = sessions
                                    .add_message(
                                        &new_id,
                                        ChatMessage::system(system_prompt.clone()),
                                    )
                                    .await;
                                let _ = sender
                                    .send(
                                        ServerMessage::Error {
                                            content: format!(
                                                "Session '{session_id}' not found. Created new session."
                                            ),
                                        }
                                        .to_text(),
                                    )
                                    .await;
                                let _ = sender
                                    .send(
                                        ServerMessage::SessionCreated {
                                            session_id: new_id,
                                        }
                                        .to_text(),
                                    )
                                    .await;
                                continue;
                            }
                        }
                        Err(e) => {
                            let _ = sender
                                .send(ServerMessage::Error { content: e }.to_text())
                                .await;
                            continue;
                        }
                    }
                }

                // Add user message to history
                if let Err(e) = sessions
                    .add_message(&session_id, ChatMessage::user(&content))
                    .await
                {
                    let _ = sender
                        .send(ServerMessage::Error { content: e }.to_text())
                        .await;
                    continue;
                }

                // Send typing indicator
                let _ = sender.send(ServerMessage::Typing.to_text()).await;

                // Get history and call provider
                let history = match sessions.get_history(&session_id).await {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = sender
                            .send(ServerMessage::Error { content: e }.to_text())
                            .await;
                        continue;
                    }
                };

                match provider.chat_with_history(&history, &model, temperature).await {
                    Ok(response) => {
                        // Add assistant response to history
                        let _ = sessions
                            .add_message(&session_id, ChatMessage::assistant(&response))
                            .await;

                        let _ = sender
                            .send(
                                ServerMessage::Message {
                                    content: response,
                                    session_id: session_id.clone(),
                                }
                                .to_text(),
                            )
                            .await;
                    }
                    Err(e) => {
                        let _ = sender
                            .send(
                                ServerMessage::Error {
                                    content: format!("Provider error: {e}"),
                                }
                                .to_text(),
                            )
                            .await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_type() {
        let json = r#"{"type":"message","content":"hello","session_id":"abc"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Message {
                content,
                session_id,
            } => {
                assert_eq!(content, "hello");
                assert_eq!(session_id, "abc");
            }
            _ => panic!("Expected Message variant"),
        }
    }

    #[test]
    fn parse_new_session_type() {
        let json = r#"{"type":"new_session"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::NewSession));
    }

    #[test]
    fn serialize_server_message() {
        let msg = ServerMessage::Message {
            content: "Hello!".into(),
            session_id: "abc".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message\""));
        assert!(json.contains("\"content\":\"Hello!\""));
    }

    #[test]
    fn serialize_typing() {
        let msg = ServerMessage::Typing;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"typing\""));
    }

    #[test]
    fn serialize_error() {
        let msg = ServerMessage::Error {
            content: "boom".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("boom"));
    }

    #[test]
    fn serialize_session_created() {
        let msg = ServerMessage::SessionCreated {
            session_id: "xyz".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"session_created\""));
        assert!(json.contains("xyz"));
    }
}
