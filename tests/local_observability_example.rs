use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use tempfile::tempdir;

#[test]
fn documented_python_observer_client_authenticates_and_consumes_sse() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind observer fixture");
    let addr = listener.local_addr().unwrap();
    let token = "phase11-observer-fixture-token-0123456789abcdef";
    let server_token = token.to_string();

    let server = thread::spawn(move || {
        for expected_path in ["/v1/agents", "/v1/events?agent_id=k7m2"] {
            let (mut stream, _) = listener.accept().expect("accept client request");
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            assert_eq!(
                request_line.trim_end(),
                format!("GET {expected_path} HTTP/1.1")
            );

            let mut authorized = false;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if line.trim_end() == format!("Authorization: Bearer {server_token}") {
                    authorized = true;
                }
            }
            assert!(authorized, "example client omitted observer bearer");

            if expected_path == "/v1/agents" {
                let body = r#"{"schema_version":1,"runtime_id":"fixture","agents":[{"id":"k7m2","first_seen_at_ms":1,"last_seen_at_ms":2,"seen_in_current_runtime":true,"active_process_count":0,"workdirs":[]}]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            } else {
                let payload = r#"{"schema_version":1,"runtime_id":"fixture","sequence":1,"emitted_at_ms":3,"event_type":"invocation_started","agent_id":"k7m2","invocation_id":7,"presentation_revision":1,"payload":{"tool_name":"exec_command"}}"#;
                let body = format!("id: 1\nevent: invocation_started\ndata: {payload}\n\n");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        }
    });

    let dir = tempdir().unwrap();
    let bearer = dir.path().join("observer-bearer");
    fs::write(&bearer, format!("{token}\n")).unwrap();
    let discovery = dir.path().join("discovery.json");
    fs::write(
        &discovery,
        format!(
            r#"{{"schema_version":1,"runtime_id":"fixture","pid":1,"start_directory":"/tmp","started_at":"2026-08-16T00:00:00Z","expires_at":null,"observability":{{"api_version":1,"presentation_version":1,"base_url":"http://{addr}","bearer_token_path":{},"history_available":true,"sse_available":true}}}}"#,
            serde_json::to_string(&bearer).unwrap()
        ),
    )
    .unwrap();

    let output = Command::new("python3")
        .arg(format!(
            "{}/examples/local_observability_client.py",
            env!("CARGO_MANIFEST_DIR")
        ))
        .arg("--discovery")
        .arg(&discovery)
        .arg("--agent")
        .arg("k7m2")
        .arg("--events")
        .arg("--max-events")
        .arg("1")
        .output()
        .expect("run documented observer client");

    server.join().expect("observer fixture server");
    assert!(
        output.status.success(),
        "client failed\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"id\": \"k7m2\""));
    assert!(stdout.contains("\"event\":\"invocation_started\""));
    assert!(stdout.contains("\"runtime_id\":\"fixture\""));
}
