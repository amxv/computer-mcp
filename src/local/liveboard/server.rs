use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path as AxumPath, RawQuery, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::TryStreamExt as _;
use serde::Serialize;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::super::LocalPaths;
use super::assets;
use super::bridge::LiveboardObserverBridge;
use super::prefs::{LiveboardPreferencesPatch, LiveboardPreferencesStore};

const PREFERENCE_BODY_LIMIT: usize = 64 * 1024;
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; worker-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'";

#[derive(Clone)]
struct LiveboardState {
    observer: Arc<LiveboardObserverBridge>,
    preferences: LiveboardPreferencesStore,
}

#[derive(Clone)]
struct SecurityState {
    expected_host: HeaderValue,
    expected_origin: HeaderValue,
}

pub(crate) struct LocalLiveboardHost {
    url: String,
    cancellation: CancellationToken,
    task: JoinHandle<Result<()>>,
}

impl LocalLiveboardHost {
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn request_shutdown(&self) {
        self.cancellation.cancel();
    }

    pub(crate) async fn shutdown(self) -> Result<()> {
        self.request_shutdown();
        self.task
            .await
            .context("Liveboard host task failed to join")??;
        Ok(())
    }
}

pub(crate) async fn start_liveboard_host(paths: &LocalPaths) -> Result<LocalLiveboardHost> {
    assets::ensure_available()?;
    let observer = Arc::new(LiveboardObserverBridge::discover(paths).await?);
    let preferences = LiveboardPreferencesStore::new(paths);
    preferences.load()?;

    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .context("failed to bind Liveboard loopback listener")?;
    let addr = listener
        .local_addr()
        .context("failed to inspect Liveboard loopback listener")?;
    if !addr.ip().is_loopback() {
        bail!("Liveboard listener bound outside loopback: {addr}");
    }

    let capability = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 24]>());
    let expected_host = HeaderValue::from_str(&addr.to_string())
        .context("Liveboard listener address was not a valid Host header")?;
    let origin = format!("http://{addr}");
    let expected_origin = HeaderValue::from_str(&origin)
        .context("Liveboard listener address was not a valid Origin header")?;
    let app = build_router(
        &capability,
        Arc::new(LiveboardState {
            observer,
            preferences,
        }),
        SecurityState {
            expected_host,
            expected_origin,
        },
    );
    let cancellation = CancellationToken::new();
    let shutdown = cancellation.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .context("Liveboard host terminated unexpectedly")
    });
    Ok(LocalLiveboardHost {
        url: format!("{origin}/{capability}/"),
        cancellation,
        task,
    })
}

fn build_router(capability: &str, state: Arc<LiveboardState>, security: SecurityState) -> Router {
    let prefix = format!("/{capability}");
    let scoped = Router::new()
        .route("/assets/{*path}", get(asset))
        .route("/preferences", get(preferences).patch(patch_preferences))
        .route("/api/status", get(proxy_status))
        .route("/api/agents", get(proxy_agents))
        .route("/api/agents/{id}", get(proxy_agent))
        .route("/api/timeline", get(proxy_timeline))
        .route(
            "/api/timeline/{presentation_id}",
            get(proxy_timeline_detail),
        )
        .route(
            "/api/timeline/{presentation_id}/checkpoints",
            get(proxy_timeline_checkpoints),
        )
        .route("/api/invocations/{id}", get(proxy_invocation))
        .route("/api/invocations/{id}/output", get(proxy_output))
        .route("/api/events", get(proxy_events))
        .with_state(state)
        .layer(DefaultBodyLimit::max(PREFERENCE_BODY_LIMIT));
    Router::new()
        .route(&prefix, get(index))
        .route(&format!("{prefix}/"), get(index))
        .nest(&prefix, scoped)
        .layer(middleware::from_fn_with_state(security, security_boundary))
}

async fn index() -> Response {
    serve_asset("index.html")
}

async fn asset(AxumPath(path): AxumPath<String>) -> Response {
    if path.is_empty() || path.contains("..") || path.contains('\\') {
        return error_response(StatusCode::NOT_FOUND, "asset was not found");
    }
    serve_asset(&format!("assets/{path}"))
}

fn serve_asset(path: &str) -> Response {
    let Some(asset) = assets::find(path) else {
        return error_response(StatusCode::NOT_FOUND, "asset was not found");
    };
    let cache_control = if assets::immutable(path) {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, assets::content_type(path))
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(Bytes::from_static(asset.bytes)))
        .expect("static Liveboard asset response must be valid")
}

async fn preferences(State(state): State<Arc<LiveboardState>>) -> Response {
    let store = state.preferences.clone();
    match tokio::task::spawn_blocking(move || store.load()).await {
        Ok(Ok(preferences)) => no_store(Json(preferences).into_response()),
        Ok(Err(error)) => internal_error(error),
        Err(error) => internal_error(error),
    }
}

async fn patch_preferences(
    State(state): State<Arc<LiveboardState>>,
    Json(patch): Json<LiveboardPreferencesPatch>,
) -> Response {
    if let Err(error) = patch.validate() {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let store = state.preferences.clone();
    match tokio::task::spawn_blocking(move || store.mutate(&patch)).await {
        Ok(Ok(preferences)) => no_store(Json(preferences).into_response()),
        Ok(Err(error)) => internal_error(error),
        Err(error) => internal_error(error),
    }
}

macro_rules! fixed_proxy {
    ($name:ident, $path:literal) => {
        async fn $name(
            State(state): State<Arc<LiveboardState>>,
            RawQuery(query): RawQuery,
        ) -> Response {
            proxy_observer(&state, $path, query.as_deref()).await
        }
    };
}

fixed_proxy!(proxy_status, "v1/status");
fixed_proxy!(proxy_agents, "v1/agents");
fixed_proxy!(proxy_timeline, "v1/timeline");
fixed_proxy!(proxy_events, "v1/events");

async fn proxy_agent(
    State(state): State<Arc<LiveboardState>>,
    AxumPath(id): AxumPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    proxy_observer(&state, &format!("v1/agents/{id}"), query.as_deref()).await
}

async fn proxy_timeline_detail(
    State(state): State<Arc<LiveboardState>>,
    AxumPath(presentation_id): AxumPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    proxy_observer(
        &state,
        &format!("v1/timeline/{presentation_id}"),
        query.as_deref(),
    )
    .await
}

async fn proxy_timeline_checkpoints(
    State(state): State<Arc<LiveboardState>>,
    AxumPath(presentation_id): AxumPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    proxy_observer(
        &state,
        &format!("v1/timeline/{presentation_id}/checkpoints"),
        query.as_deref(),
    )
    .await
}

async fn proxy_invocation(
    State(state): State<Arc<LiveboardState>>,
    AxumPath(id): AxumPath<i64>,
    RawQuery(query): RawQuery,
) -> Response {
    proxy_observer(&state, &format!("v1/invocations/{id}"), query.as_deref()).await
}

async fn proxy_output(
    State(state): State<Arc<LiveboardState>>,
    AxumPath(id): AxumPath<i64>,
    RawQuery(query): RawQuery,
) -> Response {
    proxy_observer(
        &state,
        &format!("v1/invocations/{id}/output"),
        query.as_deref(),
    )
    .await
}

async fn proxy_observer(state: &LiveboardState, path: &str, query: Option<&str>) -> Response {
    let upstream = match state.observer.get(path, query).await {
        Ok(response) => response,
        Err(error) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Local observer unavailable: {error:#}"),
            );
        }
    };
    let status = upstream.status();
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let stream = upstream.bytes_stream().map_err(|error| {
        std::io::Error::other(format!("Local observer response stream failed: {error}"))
    });
    let mut response = Response::builder()
        .status(status)
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    response
        .body(Body::from_stream(stream))
        .expect("proxied Liveboard observer response must be valid")
}

async fn security_boundary(
    State(security): State<SecurityState>,
    request: Request,
    next: Next,
) -> Response {
    if request.headers().get(header::HOST) != Some(&security.expected_host) {
        return security_headers(error_response(
            StatusCode::MISDIRECTED_REQUEST,
            "invalid Liveboard Host header",
        ));
    }
    if let Some(origin) = request.headers().get(header::ORIGIN)
        && origin != security.expected_origin
    {
        return security_headers(error_response(
            StatusCode::FORBIDDEN,
            "cross-origin Liveboard request rejected",
        ));
    }
    security_headers(next.run(request).await)
}

fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    response
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    no_store(
        (
            status,
            Json(ErrorBody {
                error: message.into(),
            }),
        )
            .into_response(),
    )
}

fn internal_error(error: impl std::fmt::Display) -> Response {
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Liveboard state error: {error}"),
    )
}
