pub mod auth;
pub mod session;
pub mod ws;

use crate::channels;
use crate::config::Config;
use crate::providers::{self, Provider};
use crate::web::session::SessionManager;
use anyhow::{Context, Result};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::Duration;

static INDEX_HTML: &str = include_str!("../../static/web/index.html");

pub struct WebAppState {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    system_prompt: String,
    sessions: Arc<SessionManager>,
    auth_token: String,
}

pub async fn run_web_server(config: Config, bind_override: Option<&str>) -> Result<()> {
    let bind = bind_override.unwrap_or(&config.web.bind);

    let provider: Arc<dyn Provider> = Arc::from(providers::create_resilient_provider(
        config.default_provider.as_deref().unwrap_or("openrouter"),
        config.api_key.as_deref(),
        &config.reliability,
    )?);

    if let Err(e) = provider.warmup().await {
        tracing::warn!("Provider warmup failed (non-fatal): {e}");
    }

    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
    let temperature = config.default_temperature;

    let workspace = config.workspace_dir.clone();
    let skills = crate::skills::load_skills(&workspace);
    let tool_descs: Vec<(&str, &str)> = vec![];

    let system_prompt = channels::build_system_prompt(
        &workspace,
        &model,
        &tool_descs,
        &skills,
        Some(&config.identity),
    );

    let sessions = Arc::new(SessionManager::new(
        config.web.max_sessions,
        config.web.session_timeout_secs,
    ));

    let state = Arc::new(WebAppState {
        provider,
        model,
        temperature,
        system_prompt,
        sessions: Arc::clone(&sessions),
        auth_token: config.web.auth_token.clone(),
    });

    // Spawn session cleanup task
    let cleanup_sessions = Arc::clone(&sessions);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let removed = cleanup_sessions.cleanup_expired().await;
            if removed > 0 {
                tracing::info!("Cleaned up {removed} expired web sessions");
            }
        }
    });

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(ws_upgrade))
        .route("/health", get(health_check))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("Failed to bind to {bind}"))?;

    let local_addr = listener.local_addr()?;
    println!("🌐 ZeroClaw Web UI started");
    println!("   URL: http://{local_addr}");
    if !config.web.auth_token.is_empty() {
        println!("   Auth: token required");
    }
    println!("   Ctrl+C to stop");

    axum::serve(listener, app)
        .await
        .context("Web server error")?;

    Ok(())
}

async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebAppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth::check_auth(&state.auth_token, &headers, &params) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    ws.on_upgrade(move |socket| {
        ws::handle_ws(
            socket,
            state.sessions.clone(),
            state.provider.clone(),
            state.model.clone(),
            state.temperature,
            state.system_prompt.clone(),
        )
    })
}

async fn health_check(
    State(state): State<Arc<WebAppState>>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "sessions": state.sessions.session_count().await,
    }))
}

async fn list_sessions(
    State(state): State<Arc<WebAppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth::check_auth(&state.auth_token, &headers, &params) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let sessions = state.sessions.list_sessions().await;
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

async fn create_session(
    State(state): State<Arc<WebAppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth::check_auth(&state.auth_token, &headers, &params) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match state.sessions.create_session().await {
        Ok(session_id) => {
            // Add system prompt
            let _ = state
                .sessions
                .add_message(
                    &session_id,
                    crate::providers::ChatMessage::system(&state.system_prompt),
                )
                .await;
            Json(serde_json::json!({ "session_id": session_id })).into_response()
        }
        Err(e) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}
