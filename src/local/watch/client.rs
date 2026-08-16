use std::fs;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt as _;
use reqwest::{Client, Response, Url};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::super::history::HistoryLiveEvent;
use super::super::observability::{
    ApiAgent, ApiAgentList, ApiInvocationDetail, ApiInvocationList, ApiStatusDocument,
};
use super::super::{
    LOCAL_OBSERVABILITY_API_VERSION, LocalPaths, LocalRuntimeDiscovery,
    PRESENTATION_SCHEMA_VERSION, load_runtime_discovery,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_millis(500);
const MIN_BEARER_LEN: usize = 32;
const RECOVERY_INVOCATION_LIMIT: usize = 100;
pub(super) const NETWORK_EVENT_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub(super) struct WatchBootstrap {
    pub discovery: LocalRuntimeDiscovery,
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
    http: Client,
    base_url: Url,
    bearer: String,
    runtime_id: String,
}

impl ObserverClient {
    pub(super) async fn discover(paths: &LocalPaths) -> Result<(Self, WatchBootstrap)> {
        let discovery = load_runtime_discovery(paths)?
            .context("Zodex Local is not running: active runtime discovery is unavailable")?;
        validate_discovery(paths, &discovery)?;

        let bearer =
            fs::read_to_string(&discovery.observability.bearer_token_path).with_context(|| {
                format!(
                    "failed to read Local observability bearer at {}",
                    discovery.observability.bearer_token_path.display()
                )
            })?;
        let bearer = bearer.trim().to_owned();
        if bearer.len() < MIN_BEARER_LEN || bearer.contains(['\r', '\n']) {
            bail!("Local observability bearer is invalid; run `zodex local setup` again");
        }

        let base_url = validate_base_url(&discovery.observability.base_url)?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to construct Local watch HTTP client")?;
        let client = Self {
            http,
            base_url,
            bearer,
            runtime_id: discovery.runtime_id.clone(),
        };
        let status = client.status().await?;
        let agents = client.current_agents().await?;
        Ok((
            client,
            WatchBootstrap {
                discovery,
                status,
                agents,
            },
        ))
    }

    pub(super) async fn status(&self) -> Result<ApiStatusDocument> {
        let status: ApiStatusDocument = self.get_json("v1/status", &[]).await?;
        self.validate_runtime(&status.runtime_id)?;
        if status.api_version != LOCAL_OBSERVABILITY_API_VERSION {
            bail!(
                "Local observability API version changed from {} to {}; restart `zodex local watch` with a matching Zodex binary",
                LOCAL_OBSERVABILITY_API_VERSION,
                status.api_version
            );
        }
        if status.presentation_version != PRESENTATION_SCHEMA_VERSION {
            bail!(
                "Local presentation version changed from {} to {}; restart `zodex local watch` with a matching Zodex binary",
                PRESENTATION_SCHEMA_VERSION,
                status.presentation_version
            );
        }
        Ok(status)
    }

    pub(super) async fn current_agents(&self) -> Result<Vec<ApiAgent>> {
        let list: ApiAgentList = self
            .get_json("v1/agents", &[("runtime", "current")])
            .await?;
        self.validate_runtime(&list.runtime_id)?;
        Ok(list.agents)
    }

    pub(super) async fn invocation(&self, id: i64) -> Result<ApiInvocationDetail> {
        let detail: ApiInvocationDetail =
            self.get_json(&format!("v1/invocations/{id}"), &[]).await?;
        self.validate_runtime(&detail.runtime_id)?;
        if detail.presentation_version != PRESENTATION_SCHEMA_VERSION {
            bail!(
                "Local invocation presentation version {} is unsupported by this watch client",
                detail.presentation_version
            );
        }
        Ok(detail)
    }

    pub(super) async fn recovery_invocations(
        &self,
        agent_id: Option<&str>,
        since_ms: i64,
    ) -> Result<ApiInvocationList> {
        let last = RECOVERY_INVOCATION_LIMIT.to_string();
        let since_ms = since_ms.to_string();
        let mut query = vec![
            ("last", last.as_str()),
            ("recovery_since_ms", since_ms.as_str()),
        ];
        if let Some(agent_id) = agent_id {
            query.push(("agent_id", agent_id));
        }
        let list: ApiInvocationList = self.get_json("v1/invocations", &query).await?;
        self.validate_runtime(&list.runtime_id)?;
        if list.presentation_version != PRESENTATION_SCHEMA_VERSION {
            bail!(
                "Local recovery presentation version {} is unsupported by this watch client",
                list.presentation_version
            );
        }
        Ok(list)
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
        let query = agent_id
            .map(|id| vec![("agent_id", id)])
            .unwrap_or_default();
        let response = ensure_success(self.get("v1/events", &query).await?).await?;
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
            let chunk = chunk.context("failed while reading Local SSE response")?;
            for event in decoder.push(&chunk)? {
                if event.runtime_id != self.runtime_id {
                    bail!(
                        "Local runtime changed from {} to {} while watch was attached",
                        self.runtime_id,
                        event.runtime_id
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

    async fn get_json<T>(&self, path: &str, query: &[(&str, &str)]) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = ensure_success(self.get(path, query).await?).await?;
        response
            .json::<T>()
            .await
            .with_context(|| format!("failed to decode Local watch response from /{path}"))
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Response> {
        let url = self
            .base_url
            .join(path)
            .with_context(|| format!("invalid Local observability path `{path}`"))?;
        let send = self
            .http
            .get(url)
            .bearer_auth(&self.bearer)
            .query(query)
            .send();
        tokio::time::timeout(REQUEST_TIMEOUT, send)
            .await
            .context("Local observability request timed out")?
            .context("failed to connect to Local observability API")
    }

    fn validate_runtime(&self, runtime_id: &str) -> Result<()> {
        if runtime_id != self.runtime_id {
            bail!(
                "Local runtime changed from {} to {}; reopen `zodex local watch`",
                self.runtime_id,
                runtime_id
            );
        }
        Ok(())
    }
}

async fn ensure_success(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Local observability request failed".to_owned());
    bail!("{message} (HTTP {status})")
}

fn validate_discovery(paths: &LocalPaths, discovery: &LocalRuntimeDiscovery) -> Result<()> {
    if discovery.observability.api_version != LOCAL_OBSERVABILITY_API_VERSION {
        bail!(
            "Local discovery advertises observability API version {}, but this Zodex supports {}",
            discovery.observability.api_version,
            LOCAL_OBSERVABILITY_API_VERSION
        );
    }
    if discovery.observability.presentation_version != PRESENTATION_SCHEMA_VERSION {
        bail!(
            "Local discovery advertises presentation version {}, but this Zodex supports {}",
            discovery.observability.presentation_version,
            PRESENTATION_SCHEMA_VERSION
        );
    }
    if discovery.observability.bearer_token_path != paths.observability_bearer_file() {
        bail!(
            "Local discovery points at an unexpected observability bearer path {}; expected {}",
            discovery.observability.bearer_token_path.display(),
            paths.observability_bearer_file().display()
        );
    }
    if !discovery.observability.sse_available {
        bail!("active Local runtime does not advertise live SSE support")
    }
    Ok(())
}

fn validate_base_url(value: &str) -> Result<Url> {
    let mut url =
        Url::parse(value).context("Local discovery contains an invalid observability URL")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("Local observability URL must be a credential-free http://127.0.0.1:<port> URL")
    }
    url.set_path("/");
    Ok(url)
}

#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
}

impl SseDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<HistoryLiveEvent>> {
        self.pending.extend_from_slice(chunk);
        let mut decoded = Vec::new();
        while let Some((end, delimiter_len)) = next_event_boundary(&self.pending) {
            let block = self.pending.drain(..end).collect::<Vec<_>>();
            self.pending.drain(..delimiter_len);
            let block =
                std::str::from_utf8(&block).context("Local SSE event was not valid UTF-8")?;
            if let Some(event) = decode_sse_block(block)? {
                decoded.push(event);
            }
        }
        Ok(decoded)
    }
}

fn next_event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    for index in 0..bytes.len() {
        if bytes.get(index..index + 2) == Some(b"\n\n") {
            return Some((index, 2));
        }
        if bytes.get(index..index + 4) == Some(b"\r\n\r\n") {
            return Some((index, 4));
        }
    }
    None
}

fn decode_sse_block(block: &str) -> Result<Option<HistoryLiveEvent>> {
    let mut data = String::new();
    for line in block.lines() {
        let Some(value) = line.strip_prefix("data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value.strip_prefix(' ').unwrap_or(value));
    }
    if data.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&data)
        .context("failed to decode Local live event")
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::{SseDecoder, validate_base_url};

    #[test]
    fn sse_decoder_handles_split_chunks_and_crlf() {
        let json = r#"{"schema_version":1,"runtime_id":"runtime","sequence":7,"emitted_at_ms":1,"event_type":"output","agent_id":"k7m2","invocation_id":3,"presentation_revision":null,"payload":{"text":"ok"}}"#;
        let payload = format!("id: 7\r\nevent: output\r\ndata: {json}\r\n\r\n");
        let split = payload.len() / 2;
        let mut decoder = SseDecoder::default();
        assert!(
            decoder
                .push(&payload.as_bytes()[..split])
                .unwrap()
                .is_empty()
        );
        let decoded = decoder.push(&payload.as_bytes()[split..]).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].sequence, 7);
        assert_eq!(decoded[0].agent_id.as_deref(), Some("k7m2"));
    }

    #[test]
    fn observer_url_must_remain_plain_loopback() {
        assert!(validate_base_url("http://127.0.0.1:43123").is_ok());
        assert!(validate_base_url("https://127.0.0.1:43123").is_err());
        assert!(validate_base_url("http://localhost:43123").is_err());
        assert!(validate_base_url("http://example.com:43123").is_err());
        assert!(validate_base_url("http://user@127.0.0.1:43123").is_err());
    }
}
