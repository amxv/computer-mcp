use std::sync::Arc;

use super::SessionManager;

impl SessionManager {
    pub async fn session_creator_agent_id(&self, session_handle: &str) -> Option<Arc<str>> {
        self.sessions
            .read()
            .await
            .get(session_handle)
            .and_then(|runtime| runtime.owned_process.as_ref())
            .and_then(|process| process.created_by.agent_id.clone())
    }
}
