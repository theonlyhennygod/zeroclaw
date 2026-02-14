use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use std::sync::RwLock;

/// Represents the current visual state of the Live Canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasState {
    /// The HTML content of the canvas (A2UI compatible).
    pub html: String,
    /// Optional CSS to inject for custom styling.
    pub css: Option<String>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            html: "<div style=\"display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;font-family:sans-serif;\">\n  <h1 style=\"color:#fff;font-size:3rem;margin-bottom:1rem;\">ZeroClaw 🦀</h1>\n  <p style=\"color:#aaa;font-size:1.2rem;\">Live Canvas Active & Ready</p>\n</div>".into(),
            css: None,
        }
    }
}

/// Manages the Live Canvas state and broadcasts updates to connected clients.
pub struct CanvasManager {
    state: RwLock<CanvasState>,
    tx: broadcast::Sender<CanvasState>,
}

impl CanvasManager {
    /// Create a new CanvasManager.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            state: RwLock::new(CanvasState::default()),
            tx,
        }
    }

    /// Update the canvas state and broadcast to all subscribers.
    pub fn set_state(&self, html: String, css: Option<String>) {
        let new_state = CanvasState { html, css };
        {
            let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
            *state = new_state.clone();
        }
        let _ = self.tx.send(new_state);
    }

    /// Append HTML to the current canvas state.
    pub fn append_html(&self, html: &str) {
        let new_state = {
            let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
            state.html.push_str(html);
            state.clone()
        };
        let _ = self.tx.send(new_state);
    }

    /// Get the current canvas state.
    pub fn get_state(&self) -> CanvasState {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Subscribe to real-time state updates.
    pub fn subscribe(&self) -> broadcast::Receiver<CanvasState> {
        self.tx.subscribe()
    }
}

impl Default for CanvasManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_initial_state() {
        let manager = CanvasManager::new();
        let state = manager.get_state();
        assert!(state.html.contains("ZeroClaw"));
    }

    #[test]
    fn test_canvas_set_state() {
        let manager = CanvasManager::new();
        manager.set_state("<h1>Update</h1>".into(), Some("body { background: red; }".into()));
        let state = manager.get_state();
        assert_eq!(state.html, "<h1>Update</h1>");
        assert_eq!(state.css, Some("body { background: red; }".into()));
    }

    #[test]
    fn test_canvas_append_html() {
        let manager = CanvasManager::new();
        manager.set_state("<p>A</p>".into(), None);
        manager.append_html("<p>B</p>");
        let state = manager.get_state();
        assert_eq!(state.html, "<p>A</p><p>B</p>");
    }

    #[tokio::test]
    async fn test_canvas_broadcast() {
        let manager = CanvasManager::new();
        let mut rx = manager.subscribe();
        
        manager.set_state("<h1>Broadcast</h1>".into(), None);
        
        let received = rx.recv().await.unwrap();
        assert_eq!(received.html, "<h1>Broadcast</h1>");
    }
}
