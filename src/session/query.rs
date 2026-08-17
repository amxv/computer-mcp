use super::{SessionCreatorContext, SessionManager};

impl SessionManager {
    pub async fn session_creator_context(
        &self,
        session_handle: &str,
    ) -> Option<SessionCreatorContext> {
        self.sessions
            .read()
            .await
            .get(session_handle)
            .and_then(|runtime| runtime.owned_process.as_ref())
            .map(|process| SessionCreatorContext {
                agent_id: process.created_by.agent_id.clone(),
                invocation_id: process.created_by.invocation_id,
            })
    }
}
