use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tempfile::tempdir;

use crate::config::Config;
use crate::protocol::{CommandStatus, ExecCommandInput, TerminationReason, WriteStdinInput};

use super::{SESSION_HANDLE_LEN, SessionManager, SessionOrigin, strip_ansi_codes};

fn test_workdir() -> String {
    std::env::current_dir()
        .expect("test current directory")
        .to_string_lossy()
        .to_string()
}

async fn start_stateful_shell(mgr: &SessionManager, cfg: &Config) -> String {
    let response = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "bash --noprofile --norc".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: Some(60_000),
            },
            cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("shell should start");

    response
        .session_handle
        .expect("stateful shell should remain running")
}

#[tokio::test]
async fn write_unknown_session_returns_error() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let err = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: "missing-handle".to_string(),
                chars: None,
                yield_time_ms: Some(50),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect_err("expected unknown session handle error");

    assert!(
        err.to_string()
            .contains("Unknown session handle: missing-handle"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn running_vs_finished_response_shape() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let finished = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "echo hi".to_string(),
                yield_time_ms: Some(2_000),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("quick command should complete");
    assert!(finished.session_id.is_none());
    assert!(finished.session_handle.is_none());
    assert_eq!(finished.exit_code, Some(0));
    assert_eq!(finished.status, CommandStatus::Exited);
    assert_eq!(finished.termination_reason, Some(TerminationReason::Exit));
    assert!(
        finished.summary.starts_with("exited 0 after "),
        "unexpected success summary: {}",
        finished.summary
    );
    assert!(
        finished.summary.ends_with('s'),
        "summary should include seconds: {}",
        finished.summary
    );

    let running = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 5".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("long command should still be running");
    assert!(running.session_id.is_some());
    assert!(running.session_handle.is_some());
    assert!(running.exit_code.is_none());
    assert_eq!(running.status, CommandStatus::Running);
    assert_eq!(running.termination_reason, None);
    let running_handle = running
        .session_handle
        .clone()
        .expect("running output should include handle");
    assert!(
        running.summary.starts_with("still running after "),
        "unexpected running summary: {}",
        running.summary
    );
    assert!(
        running
            .summary
            .contains(&format!("use session_handle {running_handle} to poll")),
        "running summary should include polling handle: {}",
        running.summary
    );

    let _ = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: running_handle,
                chars: None,
                yield_time_ms: Some(1_000),
                kill_process: Some(true),
            },
            &cfg,
        )
        .await
        .expect("cleanup should succeed");
}

#[tokio::test]
async fn ansi_codes_are_stripped_from_tool_output() {
    assert_eq!(
        strip_ansi_codes("\x1b[31mred\x1b[0m plain".to_string()),
        "red plain"
    );

    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let finished = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "printf '\\033[31mred\\033[0m plain\\n'".to_string(),
                yield_time_ms: Some(2_000),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("ansi command should complete");

    assert!(finished.output.contains("red plain"));
    assert!(
        !finished.output.contains('\u{1b}'),
        "output should not include ANSI escapes: {:?}",
        finished.output
    );
}

#[tokio::test]
async fn failed_command_summary_reports_exit_code() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let failed = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "exit 1".to_string(),
                yield_time_ms: Some(2_000),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("failing command should still return a tool output");

    assert_eq!(failed.status, CommandStatus::Exited);
    assert_eq!(failed.exit_code, Some(1));
    assert!(
        failed.summary.starts_with("exited 1 after "),
        "unexpected failure summary: {}",
        failed.summary
    );
}

#[tokio::test]
async fn state_persists_in_same_session_cd_then_pwd() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();
    let handle = start_stateful_shell(&mgr, &cfg).await;

    let _ = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle.clone(),
                chars: Some("cd /tmp\n".to_string()),
                yield_time_ms: Some(100),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("cd should succeed");

    let pwd = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle.clone(),
                chars: Some("pwd\n".to_string()),
                yield_time_ms: Some(500),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("pwd should succeed");
    assert!(pwd.output.contains("/tmp"));
    assert!(
        pwd.summary.starts_with("still running after "),
        "write_stdin polling should include a running summary: {}",
        pwd.summary
    );
    assert!(
        pwd.summary.contains("use session_handle"),
        "write_stdin polling summary should explain how to poll: {}",
        pwd.summary
    );

    let done = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle,
                chars: Some("exit\n".to_string()),
                yield_time_ms: Some(2_000),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("shell should exit");
    assert_eq!(done.exit_code, Some(0));
    assert!(done.session_handle.is_none());
    assert!(
        done.summary.starts_with("exited 0 after "),
        "write_stdin exit should include an exit summary: {}",
        done.summary
    );
}

#[tokio::test]
async fn kill_process_true_terminates_with_exit_state() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let started = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 30".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("sleep should start");
    let handle = started
        .session_handle
        .expect("expected running session handle");

    let killed = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle,
                chars: Some("echo should-be-ignored\n".to_string()),
                yield_time_ms: Some(6_000),
                kill_process: Some(true),
            },
            &cfg,
        )
        .await
        .expect("kill should succeed");

    assert!(killed.session_handle.is_none());
    assert!(killed.exit_code.is_some());
    assert_eq!(killed.status, CommandStatus::Exited);
    assert_eq!(killed.termination_reason, Some(TerminationReason::Killed));
    assert!(
        killed.summary.starts_with("killed after "),
        "kill summary should report killed state: {}",
        killed.summary
    );
    assert!(killed.output.contains("terminated by kill_process"));
    assert!(!killed.output.contains("should-be-ignored"));
}

#[tokio::test]
async fn exec_timeout_terminates_process_and_returns_notice() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config {
        default_exec_timeout_ms: 1_000,
        max_exec_timeout_ms: 1_000,
        ..Config::default()
    };

    let timed_out = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 30".to_string(),
                yield_time_ms: Some(4_000),
                workdir: test_workdir(),
                timeout_ms: Some(1_000),
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("timeout command should complete after termination");

    assert!(timed_out.session_handle.is_none());
    assert!(timed_out.exit_code.is_some());
    assert_eq!(timed_out.status, CommandStatus::Exited);
    assert_eq!(
        timed_out.termination_reason,
        Some(TerminationReason::Timeout)
    );
    assert!(
        timed_out.summary.starts_with("timed out after "),
        "timeout summary should report timeout state: {}",
        timed_out.summary
    );
    assert!(
        timed_out
            .output
            .contains("process timed out and was terminated"),
        "expected timeout notice in output: {}",
        timed_out.output
    );
}

#[tokio::test]
async fn concurrent_sessions_are_independent() {
    let mgr = Arc::new(SessionManager::new(64, 20_000));
    let cfg = Arc::new(Config::default());

    let slow_mgr = mgr.clone();
    let slow_cfg = cfg.clone();
    let slow = tokio::spawn(async move {
        slow_mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "sleep 1; echo slow".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: test_workdir(),
                    timeout_ms: None,
                },
                &slow_cfg,
                SessionOrigin::direct(),
            )
            .await
            .expect("slow command should succeed")
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let fast_mgr = mgr.clone();
    let fast_cfg = cfg.clone();
    let fast_started = Instant::now();
    let fast = tokio::spawn(async move {
        fast_mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "echo fast".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: test_workdir(),
                    timeout_ms: None,
                },
                &fast_cfg,
                SessionOrigin::direct(),
            )
            .await
            .expect("fast command should succeed")
    });

    let fast_output = fast.await.expect("fast join");
    let fast_elapsed = fast_started.elapsed();
    let slow_output = slow.await.expect("slow join");

    assert!(fast_output.output.contains("fast"));
    assert!(slow_output.output.contains("slow"));
    assert!(
        fast_elapsed < Duration::from_millis(800),
        "fast command was unexpectedly delayed: {fast_elapsed:?}"
    );
}

#[tokio::test]
async fn write_stdin_on_one_session_does_not_block_other_session_exec() {
    let mgr = Arc::new(SessionManager::new(64, 20_000));
    let cfg = Arc::new(Config::default());

    let handle = start_stateful_shell(&mgr, &cfg).await;

    let write_mgr = mgr.clone();
    let write_cfg = cfg.clone();
    let blocking_write = tokio::spawn(async move {
        write_mgr
            .write_stdin(
                WriteStdinInput {
                    session_handle: handle,
                    chars: Some("sleep 2\n".to_string()),
                    yield_time_ms: Some(2_500),
                    kill_process: Some(false),
                },
                &write_cfg,
            )
            .await
            .expect("write should succeed")
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let fast_mgr = mgr.clone();
    let fast_cfg = cfg.clone();
    let started = Instant::now();
    let fast = tokio::spawn(async move {
        fast_mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "echo concurrent-exec".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: test_workdir(),
                    timeout_ms: None,
                },
                &fast_cfg,
                SessionOrigin::direct(),
            )
            .await
            .expect("fast exec should succeed")
    });

    let fast_output = fast.await.expect("fast join");
    let fast_elapsed = started.elapsed();
    let write_output = tokio::time::timeout(Duration::from_secs(8), blocking_write)
        .await
        .expect("write_stdin on unrelated session should complete")
        .expect("write join");

    assert!(fast_output.output.contains("concurrent-exec"));
    assert!(
        fast_elapsed < Duration::from_millis(900),
        "exec was blocked by unrelated write_stdin: {fast_elapsed:?}"
    );

    let _ = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: write_output
                    .session_handle
                    .expect("session should remain running"),
                chars: Some("exit\n".to_string()),
                yield_time_ms: Some(2_000),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("cleanup should succeed");
}

#[tokio::test]
async fn same_session_operations_serialize() {
    let mgr = Arc::new(SessionManager::new(64, 20_000));
    let cfg = Arc::new(Config::default());
    let handle = start_stateful_shell(&mgr, &cfg).await;

    let first_mgr = mgr.clone();
    let first_cfg = cfg.clone();
    let first_handle = handle.clone();
    let first = tokio::spawn(async move {
        first_mgr
            .write_stdin(
                WriteStdinInput {
                    session_handle: first_handle,
                    chars: Some("sleep 1; echo first\n".to_string()),
                    yield_time_ms: Some(1_500),
                    kill_process: Some(false),
                },
                &first_cfg,
            )
            .await
            .expect("first write should succeed")
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let second_mgr = mgr.clone();
    let second_cfg = cfg.clone();
    let second = tokio::spawn(async move {
        second_mgr
            .write_stdin(
                WriteStdinInput {
                    session_handle: handle,
                    chars: Some("echo second\n".to_string()),
                    yield_time_ms: Some(400),
                    kill_process: Some(false),
                },
                &second_cfg,
            )
            .await
            .expect("second write should succeed")
    });

    let first_output = first.await.expect("first join");
    let second_output = second.await.expect("second join");

    assert!(first_output.output.contains("first"));
    assert!(second_output.output.contains("second"));

    let _ = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: second_output
                    .session_handle
                    .expect("session should still be running"),
                chars: Some("exit\n".to_string()),
                yield_time_ms: Some(2_000),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("cleanup should succeed");
}

#[tokio::test]
async fn handle_uniqueness_across_sessions() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let a = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 2".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("first session should start");
    let b = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 2".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("second session should start");

    let handle_a = a.session_handle.expect("first handle");
    let handle_b = b.session_handle.expect("second handle");
    assert_ne!(handle_a, handle_b, "session handles must be unique");
    assert_eq!(handle_a.len(), SESSION_HANDLE_LEN);
    assert_eq!(handle_b.len(), SESSION_HANDLE_LEN);
    assert!(handle_a.chars().all(|ch| ch.is_ascii_alphanumeric()));
    assert!(handle_b.chars().all(|ch| ch.is_ascii_alphanumeric()));

    let _ = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle_a,
                chars: None,
                yield_time_ms: Some(2_000),
                kill_process: Some(true),
            },
            &cfg,
        )
        .await
        .expect("cleanup a");
    let _ = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle_b,
                chars: None,
                yield_time_ms: Some(2_000),
                kill_process: Some(true),
            },
            &cfg,
        )
        .await
        .expect("cleanup b");
}

#[tokio::test]
async fn session_cleanup_after_exit_rejects_further_continuation() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();
    let handle = start_stateful_shell(&mgr, &cfg).await;

    let exited = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle.clone(),
                chars: Some("exit\n".to_string()),
                yield_time_ms: Some(2_000),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("exit should succeed");
    assert_eq!(exited.status, CommandStatus::Exited);

    let err = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle,
                chars: None,
                yield_time_ms: Some(50),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect_err("session should have been removed");
    assert!(err.to_string().contains("Unknown session handle"));
}

#[tokio::test]
async fn yielded_child_is_reaped_without_a_follow_up_poll() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();

    let running = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 0.15; printf 'reaped-without-poll\\n'".to_string(),
                yield_time_ms: Some(20),
                workdir: None,
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("command should yield while running");
    let handle = running.session_handle.expect("running session handle");

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let runtime = {
                let sessions = mgr.sessions.read().await;
                sessions.get(&handle).cloned().expect("retained session")
            };
            if runtime.inner.lock().await.reaped_exit_code.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background reaper should observe child exit");

    let completed = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle,
                chars: None,
                yield_time_ms: Some(50),
                kill_process: Some(false),
            },
            &cfg,
        )
        .await
        .expect("reaped session should retain its final response");

    assert_eq!(completed.status, CommandStatus::Exited);
    assert_eq!(completed.exit_code, Some(0));
    assert!(completed.output.contains("reaped-without-poll"));
}

#[tokio::test]
async fn background_reaper_and_poll_share_the_terminal_status() {
    let mgr = Arc::new(SessionManager::new(64, 20_000));
    let cfg = Arc::new(Config::default());

    let running = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 0.1; printf 'reaper-race-complete\\n'".to_string(),
                yield_time_ms: Some(20),
                workdir: None,
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("command should yield while running");
    let handle = running.session_handle.expect("running session handle");

    let poll_mgr = mgr.clone();
    let poll_cfg = cfg.clone();
    let completed = tokio::spawn(async move {
        poll_mgr
            .write_stdin(
                WriteStdinInput {
                    session_handle: handle,
                    chars: None,
                    yield_time_ms: Some(2_000),
                    kill_process: Some(false),
                },
                &poll_cfg,
            )
            .await
    })
    .await
    .expect("poll task should join")
    .expect("poll should observe terminal status");

    assert_eq!(completed.status, CommandStatus::Exited);
    assert_eq!(completed.exit_code, Some(0));
    assert!(completed.output.contains("reaper-race-complete"));
}

#[tokio::test]
async fn output_reports_command_cwd() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();
    let dir = tempdir().expect("tempdir");
    let workdir = dir.path().display().to_string();

    let finished = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "pwd".to_string(),
                yield_time_ms: Some(2_000),
                workdir: workdir.clone(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("pwd should complete");

    assert_eq!(finished.status, CommandStatus::Exited);
    assert_eq!(finished.cwd, workdir);
    assert!(
        finished
            .output
            .contains(dir.path().to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn invalid_workdir_is_rejected_before_command_side_effect_or_session_eviction() {
    let mgr = SessionManager::new(1, 20_000);
    let cfg = Config::default();
    let running = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 30".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: Some(60_000),
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("first session should start");
    let handle = running.session_handle.expect("running session handle");
    let dir = tempdir().expect("tempdir");
    let marker = dir.path().join("must-not-exist");
    let missing_workdir = dir.path().join("missing-workdir");

    let err = mgr
        .exec_command(
            ExecCommandInput {
                cmd: format!("touch {}", marker.display()),
                yield_time_ms: Some(2_000),
                workdir: missing_workdir.display().to_string(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect_err("missing workdir must be rejected");
    assert!(err.to_string().contains("workdir does not exist"));
    assert!(
        !marker.exists(),
        "invalid routing must have no command side effect"
    );

    let killed = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle,
                chars: None,
                yield_time_ms: Some(6_000),
                kill_process: Some(true),
            },
            &cfg,
        )
        .await
        .expect("pre-existing session must remain reachable");
    assert_eq!(killed.termination_reason, Some(TerminationReason::Killed));
}

#[tokio::test]
async fn relative_and_file_workdirs_are_rejected_before_command_side_effects() {
    let mgr = SessionManager::new(64, 20_000);
    let cfg = Config::default();
    let dir = tempdir().expect("tempdir");
    let file_workdir = dir.path().join("not-a-directory");
    std::fs::write(&file_workdir, "x").expect("seed file workdir");
    let marker = dir.path().join("must-not-exist");

    for (workdir, expected) in [
        ("relative/path".to_string(), "absolute path"),
        (file_workdir.display().to_string(), "not a directory"),
    ] {
        let err = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: format!("touch {}", marker.display()),
                    yield_time_ms: Some(2_000),
                    workdir,
                    timeout_ms: None,
                },
                &cfg,
                SessionOrigin::direct(),
            )
            .await
            .expect_err("invalid workdir must be rejected");
        assert!(
            err.to_string().contains(expected),
            "unexpected error: {err}"
        );
        assert!(!marker.exists(), "invalid workdir must not spawn command");
    }
}

#[tokio::test]
async fn full_running_session_capacity_rejects_new_work_without_orphaning() {
    let mgr = SessionManager::new(1, 20_000);
    let cfg = Config::default();
    let running = mgr
        .exec_command(
            ExecCommandInput {
                cmd: "sleep 30".to_string(),
                yield_time_ms: Some(50),
                workdir: test_workdir(),
                timeout_ms: Some(60_000),
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect("first session should start");
    let handle = running.session_handle.expect("running session handle");
    let dir = tempdir().expect("tempdir");
    let marker = dir.path().join("capacity-command-ran");

    let err = mgr
        .exec_command(
            ExecCommandInput {
                cmd: format!("touch {}", marker.display()),
                yield_time_ms: Some(2_000),
                workdir: dir.path().display().to_string(),
                timeout_ms: None,
            },
            &cfg,
            SessionOrigin::direct(),
        )
        .await
        .expect_err("full running capacity should reject new work");
    assert!(err.to_string().contains("session capacity reached"));
    assert!(!marker.exists(), "rejected work must not spawn");

    let killed = mgr
        .write_stdin(
            WriteStdinInput {
                session_handle: handle,
                chars: None,
                yield_time_ms: Some(6_000),
                kill_process: Some(true),
            },
            &cfg,
        )
        .await
        .expect("original session must remain reachable");
    assert_eq!(killed.termination_reason, Some(TerminationReason::Killed));
}

#[tokio::test]
async fn concurrent_commands_keep_explicit_workdirs_independent() {
    let mgr = Arc::new(SessionManager::new(64, 20_000));
    let cfg = Arc::new(Config::default());
    let root = tempdir().expect("root tempdir");
    let a = root.path().join("worktree-a");
    let b = root.path().join("worktree-b");
    std::fs::create_dir_all(&a).expect("create a");
    std::fs::create_dir_all(&b).expect("create b");

    let run = |dir: std::path::PathBuf, value: &'static str| {
        let mgr = mgr.clone();
        let cfg = cfg.clone();
        async move {
            mgr.exec_command(
                ExecCommandInput {
                    cmd: format!("printf '{value}' > rooted.txt; pwd"),
                    yield_time_ms: Some(2_000),
                    workdir: dir.display().to_string(),
                    timeout_ms: None,
                },
                &cfg,
                SessionOrigin::direct(),
            )
            .await
            .expect("rooted command")
        }
    };

    let (out_a, out_b) = tokio::join!(run(a.clone(), "a"), run(b.clone(), "b"));
    assert_eq!(std::fs::read_to_string(a.join("rooted.txt")).unwrap(), "a");
    assert_eq!(std::fs::read_to_string(b.join("rooted.txt")).unwrap(), "b");
    assert_eq!(out_a.cwd, a.display().to_string());
    assert_eq!(out_b.cwd, b.display().to_string());
    assert!(out_a.output.contains(a.to_string_lossy().as_ref()));
    assert!(out_b.output.contains(b.to_string_lossy().as_ref()));
}
