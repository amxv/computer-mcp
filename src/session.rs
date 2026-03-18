use std::collections::HashMap;
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
use crate::protocol::{ExecCommandInput, ToolOutput, WriteStdinInput};

const POLL_INTERVAL_MS: u64 = 30;
const TIMEOUT_NOTICE: &str = "\n[computer-mcpd] process timed out and was terminated\n";
const TERMINATE_GRACE_PERIOD_MS: u64 = 5_000;

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
    child: Child,
    pty_writer: Option<tokio::fs::File>,
    output: Arc<OutputBuffer>,
    last_used: Instant,
    deadline: Instant,
    timed_out: bool,
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

        if let Some(workdir) = input.workdir.as_deref() {
            command.current_dir(workdir);
        }

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
                pty_writer: Some(master_writer),
                child,
                output,
                last_used: Instant::now(),
                deadline: Instant::now() + Duration::from_millis(timeout_ms),
                timed_out: false,
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

        if input.kill_process.unwrap_or(false) {
            let output = {
                let session = self
                    .sessions
                    .get_mut(&input.session_id)
                    .ok_or_else(|| unknown_process_id(input.session_id))?;
                session.last_used = Instant::now();
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
                session.last_used = Instant::now();
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
            let mut finished: Option<(Arc<OutputBuffer>, i32)> = None;
            let mut running_output: Option<Arc<OutputBuffer>> = None;

            {
                let session = self
                    .sessions
                    .get_mut(&session_id)
                    .ok_or_else(|| unknown_process_id(session_id))?;
                session.last_used = Instant::now();

                maybe_force_kill(session);

                if Instant::now() >= session.deadline && !session.timed_out {
                    session.timed_out = true;
                    session.require_exit_before_return = true;
                    request_termination(session);
                    timeout_output = Some(session.output.clone());
                }

                match session.child.try_wait()? {
                    Some(status) => {
                        let code = status.code().unwrap_or(-1);
                        finished = Some((session.output.clone(), code));
                    }
                    None if started.elapsed() >= yield_for
                        && !session.require_exit_before_return =>
                    {
                        running_output = Some(session.output.clone());
                    }
                    None => {}
                }
            }

            if let Some(output) = timeout_output {
                output.append(TIMEOUT_NOTICE).await;
            }

            if let Some((output, exit_code)) = finished {
                let text = output.snapshot().await;
                self.sessions.remove(&session_id);
                return Ok(ToolOutput {
                    output: text,
                    session_id: None,
                    exit_code: Some(exit_code),
                });
            }

            if let Some(output) = running_output {
                let text = output.snapshot().await;
                return Ok(ToolOutput {
                    output: text,
                    session_id: Some(session_id),
                    exit_code: None,
                });
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }
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

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::protocol::{ExecCommandInput, WriteStdinInput};

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
        assert!(killed.output.contains("terminated by kill_process"));
    }
}
