use crate::providers::ChatMessage;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::Mutex;

/// Maximum non-system messages kept in a session's history.
const MAX_HISTORY_MESSAGES: usize = 50;

pub struct Session {
    pub id: String,
    pub history: Vec<ChatMessage>,
    pub created_at: Instant,
    pub last_activity: Instant,
}

impl Session {
    fn new(id: String) -> Self {
        Self {
            id,
            history: Vec::new(),
            created_at: Instant::now(),
            last_activity: Instant::now(),
        }
    }

    fn trim_history(&mut self) {
        let non_system: usize = self.history.iter().filter(|m| m.role != "system").count();
        if non_system > MAX_HISTORY_MESSAGES {
            let excess = non_system - MAX_HISTORY_MESSAGES;
            let mut removed = 0;
            self.history.retain(|m| {
                if m.role == "system" {
                    return true;
                }
                if removed < excess {
                    removed += 1;
                    return false;
                }
                true
            });
        }
    }
}

pub struct SessionManager {
    sessions: Mutex<HashMap<String, Session>>,
    max_sessions: usize,
    timeout_secs: u64,
}

impl SessionManager {
    pub fn new(max_sessions: usize, timeout_secs: u64) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            max_sessions,
            timeout_secs,
        }
    }

    pub async fn create_session(&self) -> Result<String, String> {
        let mut sessions = self.sessions.lock().await;
        if sessions.len() >= self.max_sessions {
            return Err(format!(
                "Maximum sessions ({}) reached",
                self.max_sessions
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        sessions.insert(id.clone(), Session::new(id.clone()));
        Ok(id)
    }

    pub async fn get_history(&self, session_id: &str) -> Result<Vec<ChatMessage>, String> {
        let mut sessions = self.sessions.lock().await;
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.last_activity = Instant::now();
                Ok(session.history.clone())
            }
            None => Err(format!("Session '{session_id}' not found")),
        }
    }

    pub async fn add_message(
        &self,
        session_id: &str,
        message: ChatMessage,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.last_activity = Instant::now();
                session.history.push(message);
                session.trim_history();
                Ok(())
            }
            None => Err(format!("Session '{session_id}' not found")),
        }
    }

    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                message_count: s.history.len(),
                age_secs: s.created_at.elapsed().as_secs(),
            })
            .collect()
    }

    pub async fn cleanup_expired(&self) -> usize {
        let mut sessions = self.sessions.lock().await;
        let before = sessions.len();
        let timeout = self.timeout_secs;
        sessions.retain(|_, s| s.last_activity.elapsed().as_secs() < timeout);
        before - sessions.len()
    }

    pub async fn session_exists(&self, session_id: &str) -> bool {
        let sessions = self.sessions.lock().await;
        sessions.contains_key(session_id)
    }

    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.lock().await;
        sessions.len()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub message_count: usize,
    pub age_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_get_session() {
        let mgr = SessionManager::new(10, 3600);
        let id = mgr.create_session().await.unwrap();
        assert!(mgr.session_exists(&id).await);
        assert_eq!(mgr.session_count().await, 1);
    }

    #[tokio::test]
    async fn add_and_retrieve_messages() {
        let mgr = SessionManager::new(10, 3600);
        let id = mgr.create_session().await.unwrap();

        mgr.add_message(&id, ChatMessage::user("Hello"))
            .await
            .unwrap();
        mgr.add_message(&id, ChatMessage::assistant("Hi there"))
            .await
            .unwrap();

        let history = mgr.get_history(&id).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
    }

    #[tokio::test]
    async fn max_sessions_enforced() {
        let mgr = SessionManager::new(2, 3600);
        mgr.create_session().await.unwrap();
        mgr.create_session().await.unwrap();
        assert!(mgr.create_session().await.is_err());
    }

    #[tokio::test]
    async fn session_not_found() {
        let mgr = SessionManager::new(10, 3600);
        assert!(mgr.get_history("nonexistent").await.is_err());
        assert!(
            mgr.add_message("nonexistent", ChatMessage::user("hi"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cleanup_expired_sessions() {
        let mgr = SessionManager::new(10, 0); // 0 second timeout = instant expiry
        let id = mgr.create_session().await.unwrap();
        assert!(mgr.session_exists(&id).await);

        // Small sleep to ensure expiry
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let removed = mgr.cleanup_expired().await;
        assert_eq!(removed, 1);
        assert!(!mgr.session_exists(&id).await);
    }

    #[tokio::test]
    async fn history_trimming() {
        let mgr = SessionManager::new(10, 3600);
        let id = mgr.create_session().await.unwrap();

        // Add a system message plus more than MAX_HISTORY_MESSAGES non-system messages
        mgr.add_message(&id, ChatMessage::system("System prompt"))
            .await
            .unwrap();
        for i in 0..60 {
            mgr.add_message(&id, ChatMessage::user(format!("msg {i}")))
                .await
                .unwrap();
        }

        let history = mgr.get_history(&id).await.unwrap();
        // System message should be preserved
        assert_eq!(history[0].role, "system");
        // Total non-system messages should be capped at MAX_HISTORY_MESSAGES
        let non_system = history.iter().filter(|m| m.role != "system").count();
        assert!(non_system <= MAX_HISTORY_MESSAGES);
    }

    #[tokio::test]
    async fn list_sessions_info() {
        let mgr = SessionManager::new(10, 3600);
        mgr.create_session().await.unwrap();
        mgr.create_session().await.unwrap();

        let list = mgr.list_sessions().await;
        assert_eq!(list.len(), 2);
    }
}
