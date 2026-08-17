use super::{build_app, build_mcp_service};
use crate::config::Config;
use crate::service::ZodexService;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::json;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

fn test_config() -> Arc<Config> {
    Arc::new(Config::default())
}

fn initialize_request() -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0.1" }
        }
    })
}

#[tokio::test]
async fn health_route_stays_public_and_stable() {
    let config = test_config();
    let service = ZodexService::new(config.clone());
    let app = build_app(config, build_mcp_service(service, CancellationToken::new()));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .expect("request build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(value["status"], "ok");
    assert_eq!(value["component"], "zodexd");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn full_sprite_router_serves_health_and_authenticated_mcp_over_plain_http() {
    let config = test_config();
    let api_key = config.api_key.clone();
    let cancellation = CancellationToken::new();
    let service = ZodexService::new(config.clone());
    let app = build_app(
        config,
        build_mcp_service(service, cancellation.child_token()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind full Sprite router test listener");
    let addr = listener.local_addr().expect("full Sprite router test addr");
    let shutdown = cancellation.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .expect("full Sprite HTTP test server");
    });

    let client = reqwest::Client::new();
    let health = client
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .expect("plain HTTP health request");
    assert_eq!(health.status(), reqwest::StatusCode::OK);
    let health_json: serde_json::Value = health.json().await.expect("health JSON");
    assert_eq!(health_json["component"], "zodexd");
    assert_eq!(health_json["version"], env!("CARGO_PKG_VERSION"));

    let mcp = client
        .post(format!("http://{addr}/mcp?key={api_key}"))
        .header("host", "localhost")
        .header("accept", "application/json, text/event-stream")
        .json(&initialize_request())
        .send()
        .await
        .expect("plain HTTP MCP initialize request");
    assert_eq!(mcp.status(), reqwest::StatusCode::OK);

    cancellation.cancel();
    server.await.expect("full Sprite HTTP test server task");
}

#[tokio::test]
async fn mcp_routes_accept_both_with_and_without_trailing_slash() {
    let config = test_config();
    let api_key = config.api_key.clone();
    let service = ZodexService::new(config.clone());
    let app = build_app(config, build_mcp_service(service, CancellationToken::new()));
    let initialize_request = initialize_request();

    for host in ["localhost", "zodex.example"] {
        for path in [
            format!("/mcp?key={api_key}"),
            format!("/mcp/?key={api_key}"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&path)
                        .header("host", host)
                        .header("content-type", "application/json")
                        .header("accept", "application/json, text/event-stream")
                        .body(Body::from(initialize_request.to_string()))
                        .expect("request build"),
                )
                .await
                .expect("request should succeed");

            let status = response.status();
            if status != StatusCode::OK {
                let body = to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("failure body");
                panic!(
                    "expected initialize to succeed for {path} with host {host}; got {status}: {}",
                    String::from_utf8_lossy(&body)
                );
            }
        }
    }
}

#[tokio::test]
async fn sprite_http_surface_rejects_wrong_mcp_key_and_has_no_v1_execution_api() {
    let config = test_config();
    let service = ZodexService::new(config.clone());
    let app = build_app(config, build_mcp_service(service, CancellationToken::new()));
    let initialize_request = initialize_request();

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp?key=wrong")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(initialize_request.to_string()))
                .expect("request build"),
        )
        .await
        .expect("wrong-key request should return a response");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    for path in ["/v1/exec-command", "/v1/write-stdin", "/v1/apply-patch"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request build"),
            )
            .await
            .expect("removed route should return a response");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{path} must stay removed"
        );
    }
}
