use anyhow::Result;
use reqwest::Response;

use super::super::observer_client::LocalObserverClient;

pub(super) struct LiveboardObserverBridge {
    client: LocalObserverClient,
}

impl LiveboardObserverBridge {
    pub(super) fn runtime_bound(client: LocalObserverClient) -> Self {
        Self { client }
    }

    pub(super) async fn get(&self, path: &str, query: Option<&str>) -> Result<Response> {
        self.client.raw_get(path, query).await
    }
}
