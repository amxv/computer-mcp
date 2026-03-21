use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
#[cfg(unix)]
use nix::errno::Errno;
#[cfg(unix)]
use nix::pty::openpty;
#[cfg(unix)]
use nix::sys::signal::{Signal, killpg};
#[cfg(unix)]
use nix::unistd::{Pid, setpgid};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::protocol::{
    CommandStatus, ExecCommandInput, TerminationReason, ToolOutput, WriteStdinInput,
};

const POLL_INTERVAL_MS: u64 = 30;
const TIMEOUT_NOTICE: &str = "\n[computer-mcpd] process timed out and was terminated\n";
const TERMINATE_GRACE_PERIOD_MS: u64 = 5_000;
const EXIT_OUTPUT_DRAIN_RETRIES: usize = 4;
const EXIT_OUTPUT_DRAIN_DELAY_MS: u64 = 10;

#[derive(Debug)]
struct OutputState {
    text: String,
    dropped_bytes: usize,
}

#[derive(Debug)]
struct OutputBuffer {
    inner: Mutex<OutputState>,
    max_chars: usize,
}

impl OutputBuffer {
    fn new(max_chars: usize) -> Self {
        Self {
            inner: Mutex::new(OutputState {
                text: String::new(),
                dropped_bytes: 0,
            }),
            max_chars,
        }
    }

    async fn append(&self, chunk: &str) {
        let mut state = self.inner.lock().await;
        state.text.push_str(chunk);

        if state.text.len() <= self.max_chars {
            return;
        }

        let overflow = state.text.len() - self.max_chars;
        let cut = next_char_boundary(&state.text, overflow);
        state.text.drain(..cut);
        state.dropped_bytes += cut;
    }

    async fn snapshot(&self) -> String {
        let state = self.inner.lock().await;
        if state.dropped_bytes == 0 {
            return state.text.clone();
        }

        format!(
            "[... {} bytes truncated ...]\n{}",
            state.dropped_bytes, state.text
        )
    }
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }

    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[derive(Debug)]
struct Session {
    pid: i32,
    last_known_cwd: String,
    child: Child,
    pty_writer: Option<tokio::fs::File>,
    output: Arc<OutputBuffer>,
    last_used: Instant,
    last_input_at: Instant,
    idle_timeout: Duration,
    timed_out: bool,
    kill_requested: bool,
    terminate_started_at: Option<Instant>,
    force_killed: bool,
    require_exit_before_return: bool,
}

#[derive(Debug)]
pub struct SessionManager {
    sessions: HashMap<u64, Session>,
    next_session_id: u64,
    max_sessions: usize,
    max_output_chars: usize,
    poll_interval: Duration,
}

impl SessionManager {
    pub fn new(max_sessions: usize, max_output_chars: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            next_session_id: 1,
            max_sessions,
            max_output_chars,
            poll_interval: Duration::from_millis(POLL_INTERVAL_MS),
        }
    }

    pub async fn exec_command(
        &mut self,
        input: ExecCommandInput,
        cfg: &Config,
    ) -> Result<ToolOutput> {
        self.evict_if_needed()?;

        let timeout_ms = cfg.clamp_exec_timeout_ms(input.timeout_ms);
        let yield_time_ms = cfg.clamp_exec_yield_ms(input.yield_time_ms);
        let now = Instant::now();

        let command_cwd = resolve_command_cwd(input.workdir.as_deref(), cfg)?;
        let command_cwd_display = command_cwd.display().to_string();

        #[cfg(unix)]
        let pty = openpty(None, None).context("failed to allocate PTY")?;
        #[cfg(unix)]
        let master_file = std::fs::File::from(pty.master);
        #[cfg(unix)]
        let slave_file = std::fs::File::from(pty.slave);
        #[cfg(unix)]
        let slave_stdin = slave_file
            .try_clone()
            .context("failed to clone PTY slave for stdin")?;
        #[cfg(unix)]
        let slave_stdout = slave_file
            .try_clone()
            .context("failed to clone PTY slave for stdout")?;

        let mut command = Command::new("/bin/bash");
        command.arg("-lc").arg(&input.cmd);

        #[cfg(unix)]
        command
            .stdin(Stdio::from(slave_stdin))
            .stdout(Stdio::from(slave_stdout))
            .stderr(Stdio::from(slave_file));

        // Put each command in its own process group so termination signals can target
        // the entire command tree (shell + children) instead of only the parent shell.
        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                setpgid(Pid::from_raw(0), Pid::from_raw(0))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                Ok(())
            });
        }

        command.current_dir(&command_cwd);
        if !cfg.agent_home.trim().is_empty() {
            command.env("HOME", &cfg.agent_home);
        }
        command.env("USER", &cfg.agent_user);
        command.env("LOGNAME", &cfg.agent_user);
        command.env("PAGER", "cat");
        command.env("GIT_PAGER", "cat");
        command.env("LESS", "FRX");
        command.env("MANPAGER", "cat");
        command.env("SYSTEMD_PAGER", "cat");

        let child = command
            .spawn()
            .with_context(|| format!("failed to spawn command: {}", input.cmd))?;

        let output = Arc::new(OutputBuffer::new(self.max_output_chars));

        #[cfg(unix)]
        let master_reader_std = master_file
            .try_clone()
            .context("failed to clone PTY master for reader")?;
        #[cfg(unix)]
        let master_writer_std = master_file;
        #[cfg(unix)]
        let master_reader = tokio::fs::File::from_std(master_reader_std);
        #[cfg(unix)]
        let master_writer = tokio::fs::File::from_std(master_writer_std);
        #[cfg(unix)]
        spawn_reader(master_reader, output.clone());

        let session_id = self.next_session_id;
        self.next_session_id += 1;
        let pid = child
            .id()
            .ok_or_else(|| anyhow!("failed to obtain child process id"))? as i32;

        self.sessions.insert(
            session_id,
            Session {
                pid,
                last_known_cwd: command_cwd_display.clone(),
                pty_writer: Some(master_writer),
                child,
                output,
                last_used: now,
                last_input_at: now,
                idle_timeout: Duration::from_millis(timeout_ms),
                timed_out: false,
                kill_requested: false,
                terminate_started_at: None,
                force_killed: false,
                require_exit_before_return: false,
            },
        );

        self.wait_for_yield_or_exit(session_id, yield_time_ms).await
    }

    pub async fn write_stdin(
        &mut self,
        input: WriteStdinInput,
        cfg: &Config,
    ) -> Result<ToolOutput> {
        let yield_time_ms = cfg.clamp_write_yield_ms(input.yield_time_ms);
        {
            let session = self
                .sessions
                .get_mut(&input.session_id)
                .ok_or_else(|| unknown_process_id(input.session_id))?;
            let now = Instant::now();
            session.last_used = now;
            session.last_input_at = now;
        }

        if input.kill_process.unwrap_or(false) {
            let output = {
                let session = self
                    .sessions
                    .get_mut(&input.session_id)
                    .ok_or_else(|| unknown_process_id(input.session_id))?;
                session.kill_requested = true;
                session.require_exit_before_return = true;
                request_termination(session);
                session.output.clone()
            };
            output
                .append("\n[computer-mcpd] process terminated by kill_process\n")
                .await;
        } else if let Some(chars) = input.chars.as_deref() {
            let mut pty_writer = {
                let session = self
                    .sessions
                    .get_mut(&input.session_id)
                    .ok_or_else(|| unknown_process_id(input.session_id))?;
                session.pty_writer.take()
            };

            if let Some(writer) = pty_writer.as_mut() {
                writer
                    .write_all(chars.as_bytes())
                    .await
                    .context("failed to write stdin")?;
                writer.flush().await.context("failed to flush stdin")?;
            }

            let session = self
                .sessions
                .get_mut(&input.session_id)
                .ok_or_else(|| unknown_process_id(input.session_id))?;
            session.pty_writer = pty_writer;
        }

        self.wait_for_yield_or_exit(input.session_id, yield_time_ms)
            .await
    }

    fn evict_if_needed(&mut self) -> Result<()> {
        while self.sessions.len() >= self.max_sessions {
            let mut oldest_any: Option<(u64, Instant)> = None;
            let mut oldest_exited: Option<(u64, Instant)> = None;

            for (&id, session) in &mut self.sessions {
                if oldest_any
                    .as_ref()
                    .map(|(_, ts)| session.last_used < *ts)
                    .unwrap_or(true)
                {
                    oldest_any = Some((id, session.last_used));
                }

                if session.child.try_wait()?.is_some()
                    && oldest_exited
                        .as_ref()
                        .map(|(_, ts)| session.last_used < *ts)
                        .unwrap_or(true)
                {
                    oldest_exited = Some((id, session.last_used));
                }
            }

            let evict_id = oldest_exited
                .map(|(id, _)| id)
                .or_else(|| oldest_any.map(|(id, _)| id));

            if let Some(id) = evict_id {
                self.sessions.remove(&id);
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn wait_for_yield_or_exit(
        &mut self,
        session_id: u64,
        yield_time_ms: u64,
    ) -> Result<ToolOutput> {
        let started = Instant::now();
        let yield_for = Duration::from_millis(yield_time_ms);

        loop {
            let mut timeout_output: Option<Arc<OutputBuffer>> = None;
            let mut finished: Option<(Arc<OutputBuffer>, i32, String, TerminationReason)> = None;
            let mut running_output: Option<(Arc<OutputBuffer>, String)> = None;

            {
                let session = self
                    .sessions
                    .get_mut(&session_id)
                    .ok_or_else(|| unknown_process_id(session_id))?;
                session.last_used = Instant::now();

                maybe_force_kill(session);
                if let Some(live_cwd) = resolve_live_cwd(session.pid) {
                    session.last_known_cwd = live_cwd;
                }

                if session.last_input_at.elapsed() >= session.idle_timeout && !session.timed_out {
                    session.timed_out = true;
                    session.require_exit_before_return = true;
                    request_termination(session);
                    timeout_output = Some(session.output.clone());
                }

                match session.child.try_wait()? {
                    Some(status) => {
                        let code = status.code().unwrap_or(-1);
                        let termination_reason = if session.timed_out {
                            TerminationReason::Timeout
                        } else if session.kill_requested || session.force_killed {
                            TerminationReason::Killed
                        } else {
                            TerminationReason::Exit
                        };
                        finished = Some((
                            session.output.clone(),
                            code,
                            session.last_known_cwd.clone(),
                            termination_reason,
                        ));
                    }
                    None if started.elapsed() >= yield_for
                        && !session.require_exit_before_return =>
                    {
                        running_output =
                            Some((session.output.clone(), session.last_known_cwd.clone()));
                    }
                    None => {}
                }
            }

            if let Some(output) = timeout_output {
                output.append(TIMEOUT_NOTICE).await;
            }

            if let Some((output, exit_code, cwd, termination_reason)) = finished {
                let text = snapshot_output_after_exit(&output).await;
                self.sessions.remove(&session_id);
                return Ok(ToolOutput {
                    output: text,
                    status: CommandStatus::Exited,
                    cwd,
                    session_id: None,
                    exit_code: Some(exit_code),
                    termination_reason: Some(termination_reason),
                });
            }

            if let Some((output, cwd)) = running_output {
                let text = output.snapshot().await;
                return Ok(ToolOutput {
                    output: text,
                    status: CommandStatus::Running,
                    cwd,
                    session_id: Some(session_id),
                    exit_code: None,
                    termination_reason: None,
                });
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

fn resolve_command_cwd(requested_workdir: Option<&str>, cfg: &Config) -> Result<PathBuf> {
    if let Some(workdir) = requested_workdir {
        return Ok(PathBuf::from(workdir));
    }

    if !cfg.default_workdir.trim().is_empty() {
        let path = PathBuf::from(&cfg.default_workdir);
        if path.is_dir() {
            return Ok(path);
        }
    }

    if !cfg.agent_home.trim().is_empty() {
        let path = PathBuf::from(&cfg.agent_home);
        if path.is_dir() {
            return Ok(path);
        }
    }

    std::env::current_dir().context("failed to resolve current directory")
}

fn request_termination(session: &mut Session) {
    if session.terminate_started_at.is_some() {
        return;
    }

    session.terminate_started_at = Some(Instant::now());
    #[cfg(unix)]
    {
        let _ = signal_process_group(session.pid, Signal::SIGTERM);
    }
    #[cfg(not(unix))]
    {
        let _ = session.child.start_kill();
    }
}

async fn snapshot_output_after_exit(output: &Arc<OutputBuffer>) -> String {
    let mut snapshot = output.snapshot().await;
    for _ in 0..EXIT_OUTPUT_DRAIN_RETRIES {
        tokio::time::sleep(Duration::from_millis(EXIT_OUTPUT_DRAIN_DELAY_MS)).await;
        let refreshed = output.snapshot().await;
        if refreshed == snapshot {
            break;
        }
        snapshot = refreshed;
    }
    snapshot
}

fn maybe_force_kill(session: &mut Session) {
    let Some(started) = session.terminate_started_at else {
        return;
    };
    if session.force_killed {
        return;
    }
    if started.elapsed() < Duration::from_millis(TERMINATE_GRACE_PERIOD_MS) {
        return;
    }

    session.force_killed = true;
    #[cfg(unix)]
    {
        let _ = signal_process_group(session.pid, Signal::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = session.child.start_kill();
    }
}

#[cfg(unix)]
fn signal_process_group(pid: i32, signal: Signal) -> Result<()> {
    match killpg(Pid::from_raw(pid), signal) {
        Ok(_) => Ok(()),
        Err(Errno::ESRCH) => Ok(()),
        Err(e) => Err(anyhow!(
            "failed to send {signal:?} to process group {pid}: {e}"
        )),
    }
}

fn spawn_reader<R>(mut reader: R, output: Arc<OutputBuffer>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = [0_u8; 8192];
        loop {
            let read = match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };

            let chunk = String::from_utf8_lossy(&buf[..read]);
            output.append(&chunk).await;
        }
    });
}

fn unknown_process_id(session_id: u64) -> anyhow::Error {
    anyhow!("Unknown process id: {}", session_id)
}

fn resolve_live_cwd(pid: i32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let target_pgrp = read_proc_pgrp(pid)?;
        let mut best: Option<(i32, String)> = None;

        let proc_entries = std::fs::read_dir("/proc").ok()?;
        for entry in proc_entries {
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name();
            let raw = name.to_string_lossy();
            if !raw.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }

            let proc_pid = match raw.parse::<i32>() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if read_proc_pgrp(proc_pid) != Some(target_pgrp) {
                continue;
            }

            let Some(cwd) = read_proc_cwd(proc_pid) else {
                continue;
            };
            if best
                .as_ref()
                .map(|(best_pid, _)| proc_pid > *best_pid)
                .unwrap_or(true)
            {
                best = Some((proc_pid, cwd));
            }
        }

        if let Some((_, cwd)) = best {
            return Some(cwd);
        }

        return read_proc_cwd(pid);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "linux")]
fn read_proc_cwd(pid: i32) -> Option<String> {
    let path = format!("/proc/{pid}/cwd");
    let cwd = std::fs::read_link(path).ok()?;
    Some(cwd.display().to_string())
}

#[cfg(target_os = "linux")]
fn read_proc_pgrp(pid: i32) -> Option<i32> {
    let stat_path = format!("/proc/{pid}/stat");
    let raw = std::fs::read_to_string(stat_path).ok()?;
    let (_, after_comm) = raw.rsplit_once(") ")?;
    let mut fields = after_comm.split_whitespace();
    let _state = fields.next()?;
    let _ppid = fields.next()?;
    let pgrp = fields.next()?.parse::<i32>().ok()?;
    Some(pgrp)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::config::Config;
    use crate::protocol::{CommandStatus, ExecCommandInput, TerminationReason, WriteStdinInput};
    use tempfile::tempdir;

    use super::SessionManager;

    async fn start_stateful_shell(mgr: &mut SessionManager, cfg: &Config) -> u64 {
        let response = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "bash --noprofile --norc".to_string(),
                    yield_time_ms: Some(50),
                    workdir: None,
                    timeout_ms: Some(60_000),
                },
                cfg,
            )
            .await
            .expect("shell should start");

        response
            .session_id
            .expect("stateful shell should remain running")
    }

    #[tokio::test]
    async fn write_unknown_session_returns_error() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();

        let err = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: 404,
                    chars: None,
                    yield_time_ms: Some(50),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect_err("expected unknown session id error");

        assert!(err.to_string().contains("Unknown process id: 404"));
    }

    #[tokio::test]
    async fn kill_unknown_session_returns_error() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();

        let err = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: 505,
                    chars: Some("echo hi\n".to_string()),
                    yield_time_ms: Some(50),
                    kill_process: Some(true),
                },
                &cfg,
            )
            .await
            .expect_err("expected unknown session id error");

        assert!(err.to_string().contains("Unknown process id: 505"));
    }

    #[tokio::test]
    async fn running_vs_finished_response_shape() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();

        let finished = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "echo hi".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: None,
                    timeout_ms: None,
                },
                &cfg,
            )
            .await
            .expect("quick command should complete");
        assert!(finished.session_id.is_none());
        assert_eq!(finished.exit_code, Some(0));
        assert_eq!(finished.status, CommandStatus::Exited);
        assert_eq!(finished.termination_reason, Some(TerminationReason::Exit));
        assert!(!finished.cwd.is_empty());

        let running = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "sleep 5".to_string(),
                    yield_time_ms: Some(50),
                    workdir: None,
                    timeout_ms: None,
                },
                &cfg,
            )
            .await
            .expect("long command should still be running");
        assert!(running.session_id.is_some());
        assert!(running.exit_code.is_none());
        assert_eq!(running.status, CommandStatus::Running);
        assert_eq!(running.termination_reason, None);
        assert!(!running.cwd.is_empty());

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: running.session_id.expect("session id should exist"),
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
    async fn output_reports_command_cwd() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();
        let dir = tempdir().expect("tempdir");
        let workdir = dir.path().display().to_string();

        let finished = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "pwd".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: Some(workdir.clone()),
                    timeout_ms: None,
                },
                &cfg,
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
    async fn defaults_to_config_default_workdir_when_input_workdir_missing() {
        let mut mgr = SessionManager::new(64, 20_000);
        let mut cfg = Config::default();
        let dir = tempdir().expect("tempdir");
        cfg.default_workdir = dir.path().display().to_string();

        let finished = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "pwd".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: None,
                    timeout_ms: None,
                },
                &cfg,
            )
            .await
            .expect("pwd should complete");

        assert_eq!(finished.status, CommandStatus::Exited);
        assert_eq!(finished.cwd, cfg.default_workdir);
        assert!(
            finished
                .output
                .contains(dir.path().to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn falls_back_when_default_workdir_is_missing() {
        let mut mgr = SessionManager::new(64, 20_000);
        let mut cfg = Config::default();
        let dir = tempdir().expect("tempdir");
        cfg.agent_home = dir.path().display().to_string();
        cfg.default_workdir = dir.path().join("missing-workspace").display().to_string();

        let finished = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "pwd".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: None,
                    timeout_ms: None,
                },
                &cfg,
            )
            .await
            .expect("pwd should complete");

        assert_eq!(finished.status, CommandStatus::Exited);
        assert_eq!(finished.cwd, cfg.agent_home);
        assert!(
            finished
                .output
                .contains(dir.path().to_string_lossy().as_ref())
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn poll_reports_live_cwd_after_cd() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();
        let sid = start_stateful_shell(&mut mgr, &cfg).await;
        let dir = tempdir().expect("tempdir");
        let workdir = dir.path().display().to_string();

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some(format!("cd {workdir}\n")),
                    yield_time_ms: Some(100),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("cd should succeed");

        let mut observed_match = false;
        for _ in 0..8 {
            let poll = mgr
                .write_stdin(
                    WriteStdinInput {
                        session_id: sid,
                        chars: None,
                        yield_time_ms: Some(100),
                        kill_process: Some(false),
                    },
                    &cfg,
                )
                .await
                .expect("poll should succeed");

            assert_eq!(poll.status, CommandStatus::Running);
            if poll.cwd == workdir {
                observed_match = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: None,
                    yield_time_ms: Some(2_000),
                    kill_process: Some(true),
                },
                &cfg,
            )
            .await
            .expect("cleanup should succeed");

        assert!(
            observed_match,
            "expected live cwd {workdir} to be reported after cd"
        );
    }

    #[tokio::test]
    async fn state_persists_in_same_session_cd_then_pwd() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();
        let sid = start_stateful_shell(&mut mgr, &cfg).await;

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
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
                    session_id: sid,
                    chars: Some("pwd\n".to_string()),
                    yield_time_ms: Some(500),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("pwd should succeed");
        assert!(pwd.output.contains("/tmp"));

        let done = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("exit\n".to_string()),
                    yield_time_ms: Some(2_000),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("shell should exit");
        assert_eq!(done.exit_code, Some(0));
        assert!(done.session_id.is_none());
    }

    #[tokio::test]
    async fn env_persists_in_same_session_export_then_read() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();
        let sid = start_stateful_shell(&mut mgr, &cfg).await;

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("export MCP_TEST_VAR=hello-session\n".to_string()),
                    yield_time_ms: Some(100),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("export should succeed");

        let read = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("printf '%s\\n' \"$MCP_TEST_VAR\"\n".to_string()),
                    yield_time_ms: Some(500),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("read should succeed");
        assert!(read.output.contains("hello-session"));

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("exit\n".to_string()),
                    yield_time_ms: Some(2_000),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("shell exit should succeed");
    }

    #[tokio::test]
    async fn session_is_attached_to_tty() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();
        let sid = start_stateful_shell(&mut mgr, &cfg).await;

        let tty_check = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some(
                        "test -t 0 && test -t 1 && echo tty-ok || echo tty-no\n".to_string(),
                    ),
                    yield_time_ms: Some(500),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("tty check should run");

        assert!(
            tty_check.output.contains("tty-ok"),
            "expected PTY-backed stdin/stdout, got output: {}",
            tty_check.output
        );

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("exit\n".to_string()),
                    yield_time_ms: Some(2_000),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("shell exit should succeed");
    }

    #[tokio::test]
    async fn kill_process_true_terminates_with_exit_state() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();

        let started = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "sleep 30".to_string(),
                    yield_time_ms: Some(50),
                    workdir: None,
                    timeout_ms: None,
                },
                &cfg,
            )
            .await
            .expect("sleep should start");
        let sid = started.session_id.expect("expected running session");

        let killed = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("echo should-be-ignored\n".to_string()),
                    yield_time_ms: Some(6_000),
                    kill_process: Some(true),
                },
                &cfg,
            )
            .await
            .expect("kill should succeed");

        assert!(killed.session_id.is_none());
        assert!(killed.exit_code.is_some());
        assert_eq!(killed.status, CommandStatus::Exited);
        assert_eq!(killed.termination_reason, Some(TerminationReason::Killed));
        assert!(killed.output.contains("terminated by kill_process"));
        assert!(!killed.output.contains("should-be-ignored"));
    }

    #[tokio::test]
    async fn exec_timeout_terminates_process_and_returns_notice() {
        let mut mgr = SessionManager::new(64, 20_000);
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
                    workdir: None,
                    timeout_ms: Some(1_000),
                },
                &cfg,
            )
            .await
            .expect("timeout command should complete after termination");

        assert!(timed_out.session_id.is_none());
        assert!(timed_out.exit_code.is_some());
        assert_eq!(timed_out.status, CommandStatus::Exited);
        assert_eq!(
            timed_out.termination_reason,
            Some(TerminationReason::Timeout)
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
    async fn idle_timeout_resets_on_write_stdin() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config {
            default_exec_timeout_ms: 1_000,
            max_exec_timeout_ms: 1_000,
            ..Config::default()
        };
        let sid = start_stateful_shell(&mut mgr, &cfg).await;

        tokio::time::sleep(Duration::from_millis(700)).await;
        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: None,
                    yield_time_ms: Some(50),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("poll should succeed");

        tokio::time::sleep(Duration::from_millis(700)).await;
        let after_reset = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: Some("echo still-alive\n".to_string()),
                    yield_time_ms: Some(500),
                    kill_process: Some(false),
                },
                &cfg,
            )
            .await
            .expect("session should still be alive after idle reset");

        assert_eq!(after_reset.status, CommandStatus::Running);
        assert_eq!(after_reset.termination_reason, None);
        assert!(after_reset.output.contains("still-alive"));

        let _ = mgr
            .write_stdin(
                WriteStdinInput {
                    session_id: sid,
                    chars: None,
                    yield_time_ms: Some(2_000),
                    kill_process: Some(true),
                },
                &cfg,
            )
            .await
            .expect("cleanup should succeed");
    }

    #[tokio::test]
    async fn exec_sets_non_interactive_pager_env_defaults() {
        let mut mgr = SessionManager::new(64, 20_000);
        let cfg = Config::default();

        let finished = mgr
            .exec_command(
                ExecCommandInput {
                    cmd: "printf '%s|%s|%s|%s|%s' \"$PAGER\" \"$GIT_PAGER\" \"$LESS\" \"$MANPAGER\" \"$SYSTEMD_PAGER\"".to_string(),
                    yield_time_ms: Some(2_000),
                    workdir: None,
                    timeout_ms: None,
                },
                &cfg,
            )
            .await
            .expect("env command should complete");

        assert_eq!(finished.status, CommandStatus::Exited);
        assert!(finished.output.contains("cat|cat|FRX|cat|cat"));
    }
}
