use std::time::Duration;

use anyhow::{Result, bail};
use futures_util::StreamExt as _;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::super::LocalPaths;
use super::super::history::{HISTORY_LIVE_EVENT_SCHEMA_VERSION, HistoryLiveEvent};
use super::super::observability::{
    ApiAgent, ApiInvocationDetail, ApiInvocationList, ApiStatusDocument, ApiTimelineDetail,
};
use super::super::observer_client::{LocalObserverClient, ObserverEventSelection};
use super::sse::SseDecoder;

const RECONNECT_DELAY: Duration = Duration::from_millis(500);
const RECOVERY_INVOCATION_LIMIT: usize = 100;
pub(super) const NETWORK_EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub(super) struct WatchBootstrap {
    pub discovery: super::super::LocalRuntimeDiscovery,
    pub status: ApiStatusDocument,
    pub agents: Vec<ApiAgent>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum WatchNetworkEvent {
    Connected(u64),
    Live(u64, HistoryLiveEvent),
    Disconnected(u64, String),
}

#[derive(Clone)]
pub(super) struct ObserverClient {
    inner: LocalObserverClient,
}

impl ObserverClient {
    pub(super) async fn discover(paths: &LocalPaths) -> Result<(Self, WatchBootstrap)> {
        let (inner, bootstrap) = LocalObserverClient::discover(paths).await?;
        Ok((
            Self { inner },
            WatchBootstrap {
                discovery: bootstrap.discovery,
                status: bootstrap.status,
                agents: bootstrap.agents,
            },
        ))
    }

    pub(super) async fn status(&self) -> Result<ApiStatusDocument> {
        self.inner.status().await
    }

    pub(super) async fn current_agents(&self) -> Result<Vec<ApiAgent>> {
        self.inner.current_agents().await
    }

    pub(super) async fn invocation(&self, id: i64) -> Result<ApiInvocationDetail> {
        self.inner.invocation(id).await
    }

    pub(super) async fn recovery_invocations(
        &self,
        agent_id: Option<&str>,
        since_ms: i64,
    ) -> Result<ApiInvocationList> {
        self.inner
            .recovery_invocations(agent_id, since_ms, RECOVERY_INVOCATION_LIMIT)
            .await
    }

    pub(super) async fn timeline_detail(&self, presentation_id: &str) -> Result<ApiTimelineDetail> {
        self.inner.timeline_detail(presentation_id).await
    }

    pub(super) fn spawn_event_stream(
        &self,
        generation: u64,
        agent_id: Option<String>,
        sender: mpsc::Sender<WatchNetworkEvent>,
        cancellation: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let client = self.clone();
        tokio::spawn(async move {
            while !cancellation.is_cancelled() {
                match client
                    .stream_once(generation, agent_id.as_deref(), &sender, &cancellation)
                    .await
                {
                    Ok(()) if cancellation.is_cancelled() => break,
                    Ok(()) => {
                        let _ = sender
                            .send(WatchNetworkEvent::Disconnected(
                                generation,
                                "Local live stream ended; reconnecting".to_owned(),
                            ))
                            .await;
                    }
                    Err(error) => {
                        let _ = sender
                            .send(WatchNetworkEvent::Disconnected(
                                generation,
                                format!("Local live stream disconnected: {error:#}"),
                            ))
                            .await;
                    }
                }
                tokio::select! {
                    _ = cancellation.cancelled() => break,
                    _ = tokio::time::sleep(RECONNECT_DELAY) => {}
                }
            }
        })
    }

    async fn stream_once(
        &self,
        generation: u64,
        agent_id: Option<&str>,
        sender: &mpsc::Sender<WatchNetworkEvent>,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        let response = self
            .inner
            .events_response(agent_id, &ObserverEventSelection::default())
            .await?;
        if sender
            .send(WatchNetworkEvent::Connected(generation))
            .await
            .is_err()
        {
            return Ok(());
        }
        let mut stream = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        loop {
            let next = tokio::select! {
                _ = cancellation.cancelled() => return Ok(()),
                next = stream.next() => next,
            };
            let Some(chunk) = next else {
                return Ok(());
            };
            let chunk = chunk.map_err(anyhow::Error::from)?;
            for event in decoder.push(&chunk)? {
                if event.runtime_id != self.inner.runtime_id() {
                    bail!(
                        "Local runtime changed from {} to {} while watch was attached",
                        self.inner.runtime_id(),
                        event.runtime_id
                    );
                }
                if event.schema_version != HISTORY_LIVE_EVENT_SCHEMA_VERSION {
                    bail!(
                        "Local live-event schema version changed from {} to {}; reopen `zodex local watch` with a matching Zodex binary",
                        HISTORY_LIVE_EVENT_SCHEMA_VERSION,
                        event.schema_version
                    );
                }
                if sender
                    .send(WatchNetworkEvent::Live(generation, event))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
        }
    }
}
