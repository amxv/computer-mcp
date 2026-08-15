use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvocationContext {
    pub correlation_id: Option<Arc<str>>,
    pub provider: Option<ProviderCallMetadata>,
    pub agent_id: Option<Arc<str>>,
}

impl InvocationContext {
    pub fn with_correlation_id(mut self, value: impl Into<Arc<str>>) -> Self {
        self.correlation_id = Some(value.into());
        self
    }

    pub fn with_provider(mut self, provider: ProviderCallMetadata) -> Self {
        self.provider = Some(provider);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCallMetadata {
    pub kind: Arc<str>,
    pub session_key: Arc<str>,
}

impl ProviderCallMetadata {
    pub fn new(kind: impl Into<Arc<str>>, session_key: impl Into<Arc<str>>) -> Self {
        Self {
            kind: kind.into(),
            session_key: session_key.into(),
        }
    }
}
