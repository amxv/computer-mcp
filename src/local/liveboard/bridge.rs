use anyhow::{Context, Result};
use reqwest::Response;
use tokio::sync::RwLock;

use super::super::LocalPaths;
use super::super::observer_client::LocalObserverClient;

pub(super) struct LiveboardObserverBridge {
    paths: LocalPaths,
    client: RwLock<LocalObserverClient>,
}

impl LiveboardObserverBridge {
    pub(super) async fn discover(paths: &LocalPaths) -> Result<Self> {
        let (client, _bootstrap) = LocalObserverClient::discover(paths).await?;
        Ok(Self {
            paths: paths.clone(),
            client: RwLock::new(client),
        })
    }

    pub(super) async fn get(&self, path: &str, query: Option<&str>) -> Result<Response> {
        let client = self.client.read().await.clone();
        match client.raw_get(path, query).await {
            Ok(response) => Ok(response),
            Err(first_error) => {
                let (replacement, _bootstrap) = LocalObserverClient::discover(&self.paths)
                    .await
                    .with_context(|| {
                        format!(
                            "Local observer is unavailable after connection failure: {first_error:#}"
                        )
                    })?;
                let response = replacement.raw_get(path, query).await?;
                *self.client.write().await = replacement;
                Ok(response)
            }
        }
    }
}
