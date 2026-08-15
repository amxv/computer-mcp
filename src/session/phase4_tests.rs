use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::invocation::{InvocationContext, ProviderCallMetadata};
use crate::protocol::{CommandStatus, ExecCommandInput, TerminationReason, WriteStdinInput};

use super::{
    ProcessInspector, SessionManager, SessionOrigin, SessionOutputChunk, SessionOutputObserver,
    SessionRuntimePolicy, SystemProcessInspector,
};

fn workdir() -> String {
    std::env::current_dir().unwrap().display().to_string()
}

fn local_environment() -> Vec<(OsString, OsString)> {
    [
        ("HOME", "/tmp/zodex-local-home"),
        ("USER", "local-user"),
        ("LOGNAME", "local-logname"),
        ("PATH", "/usr/bin:/bin"),
        ("LOCAL_EXPORTED_VALUE", "captured-value"),
        ("PAGER", "less"),
        ("GIT_PAGER", "less"),
        ("MANPAGER", "less"),
        ("LESS", "-R"),
        ("SYSTEMD_PAGER", "less"),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect()
}

fn local_policy() -> SessionRuntimePolicy {
    SessionRuntimePolicy::local("/bin/sh", local_environment()).unwrap()
}

#[tokio::test]
async fn local_profile_uses_captured_user_environment_and_overlays_pager_guards() {
    let mgr = SessionManager::with_policy(8, 20_000, local_policy());
    let cfg = Config {
        agent_user: "sprite-user-must-not-leak".to_string(),
        agent_home: "/sprite/home/must-not-leak".to_string(),
        ..Config::default()
    };
    let output = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\\n' \"$HOME\" \"$USER\" \"$LOGNAME\" \"$LOCAL_EXPORTED_VALUE\" \"$PAGER\" \"$GIT_PAGER\" \"$MANPAGER\" \"$LESS\" \"$SYSTEMD_PAGER\"".to_string(),
                yield_time_ms: Some(2_000),
                workdir: workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .unwrap();

    assert_eq!(output.status, CommandStatus::Exited);
    assert!(output.output.contains(
        "/tmp/zodex-local-home|local-user|local-logname|captured-value|cat|cat|cat|FRX|cat"
    ));
    assert!(!output.output.contains("sprite-user-must-not-leak"));
    assert!(!output.output.contains("/sprite/home/must-not-leak"));
}

#[derive(Default)]
struct CapturingOutputObserver {
    chunks: Mutex<Vec<SessionOutputChunk>>,
}

impl SessionOutputObserver for CapturingOutputObserver {
    fn observe_output(&self, chunk: SessionOutputChunk) {
        self.chunks.lock().unwrap().push(chunk);
    }
}

#[tokio::test]
async fn output_observer_receives_full_stream_with_invocation_context_while_tool_output_is_bounded()
{
    let observer = Arc::new(CapturingOutputObserver::default());
    let policy = local_policy().with_output_observer(observer.clone());
    let mgr = SessionManager::with_policy(8, 40, policy);
    let cfg = Config::default();
    let invocation = InvocationContext::default()
        .with_correlation_id("phase4-invocation")
        .with_provider(ProviderCallMetadata::new(
            "openai/session",
            "phase4-provider-session",
        ));
    let output = mgr
        .exec_command_with_context(
            ExecCommandInput {
                cmd: "printf '%0500d\\n' 0 | tr '0' x".to_string(),
                yield_time_ms: Some(2_000),
                workdir: workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::mcp(None),
            invocation.clone(),
        )
        .await
        .unwrap();

    assert!(output.output.contains("bytes truncated"));
    let chunks = observer.chunks.lock().unwrap();
    let full = chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect::<String>();
    assert!(
        full.matches('x').count() >= 500,
        "full output was not observed: {full:?}"
    );
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|chunk| chunk.invocation == invocation));
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.session_handle.len() == super::SESSION_HANDLE_LEN)
    );
}

#[tokio::test]
async fn whole_runtime_shutdown_preempts_a_long_yield_write_and_closes_admission() {
    let policy = local_policy().with_shutdown_grace(Duration::from_millis(250));
    let mgr = Arc::new(SessionManager::with_policy(8, 20_000, policy));
    let cfg = Arc::new(Config::default());
    let started = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "bash --noprofile --norc".to_string(),
                yield_time_ms: Some(50),
                workdir: workdir(),
                timeout_ms: Some(60_000),
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .unwrap();
    let handle = started.session_handle.unwrap();

    let poll_mgr = mgr.clone();
    let poll_cfg = cfg.clone();
    let poll = tokio::spawn(async move {
        poll_mgr
            .write_stdin(
                WriteStdinInput {
                    session_handle: handle,
                    chars: Some("sleep 30\n".to_string()),
                    yield_time_ms: Some(60_000),
                    kill_process: Some(false),
                },
                &poll_cfg,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let began = Instant::now();
    let shutdown = mgr.shutdown_all().await.unwrap();
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "runtime shutdown waited behind normal write_stdin serialization: {:?}",
        began.elapsed()
    );
    assert_eq!(shutdown.sessions_signaled, 1);
    assert!(!mgr.accepting_new_sessions());

    let terminal = tokio::time::timeout(Duration::from_secs(2), poll)
        .await
        .expect("in-flight write should converge promptly")
        .expect("poll task should join")
        .expect("poll should return truthful terminal output");
    assert_eq!(terminal.status, CommandStatus::Exited);
    assert_eq!(terminal.termination_reason, Some(TerminationReason::Killed));

    let error = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "echo must-not-run".to_string(),
                yield_time_ms: Some(100),
                workdir: workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect_err("shutdown admission must stay closed");
    assert!(error.to_string().contains("stopping"));
}

#[tokio::test]
async fn multi_session_shutdown_uses_one_shared_grace_window() {
    let grace = Duration::from_secs(1);
    let policy = local_policy().with_shutdown_grace(grace);
    let mgr = SessionManager::with_policy(8, 20_000, policy);
    let cfg = Config::default();

    for _ in 0..3 {
        let started = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "trap '' TERM; while :; do sleep 1; done".to_string(),
                    yield_time_ms: Some(50),
                    workdir: workdir(),
                    timeout_ms: Some(60_000),
                },
                &cfg,
                SessionOrigin::direct(),
            )
            .await
            .unwrap();
        assert_eq!(started.status, CommandStatus::Running);
    }

    let began = Instant::now();
    let shutdown = mgr.shutdown_all().await.unwrap();
    let elapsed = began.elapsed();
    assert_eq!(shutdown.sessions_signaled, 3);
    assert_eq!(shutdown.sessions_force_killed, 3);
    assert!(
        elapsed < Duration::from_millis(2_800),
        "three sessions appear to have consumed serial grace windows: {elapsed:?}"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn whole_runtime_shutdown_sweeps_discoverable_descendants_outside_the_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("escaped.pid");
    let policy = local_policy().with_shutdown_grace(Duration::from_millis(300));
    let mgr = SessionManager::with_policy(8, 20_000, policy);
    let cfg = Config::default();
    let started = mgr
        .exec_command(
            ExecCommandInput {
                cmd: format!(
                    "trap '' TERM; setsid sh -c 'trap \"\" TERM; echo $$ > {}; while :; do sleep 1; done' & while :; do sleep 1; done",
                    pid_file.display()
                ),
                yield_time_ms: Some(100),
                workdir: dir.path().display().to_string(),
                timeout_ms: Some(60_000),
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .unwrap();
    assert_eq!(started.status, CommandStatus::Running);

    let deadline = Instant::now() + Duration::from_secs(2);
    while !pid_file.exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let escaped_pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let inspector = SystemProcessInspector;
    assert!(inspector.identity(escaped_pid).unwrap().is_some());

    let shutdown = mgr.shutdown_all().await.unwrap();
    assert!(shutdown.descendants_signaled >= 1);
    assert!(shutdown.descendants_force_killed >= 1);

    let deadline = Instant::now() + Duration::from_secs(2);
    while inspector.identity(escaped_pid).unwrap().is_some() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        inspector.identity(escaped_pid).unwrap().is_none(),
        "discoverable escaped descendant {escaped_pid} survived whole-runtime shutdown"
    );
}
