use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::process::Command;

use crate::config::Config;
use crate::invocation::InvocationContext;

use super::{ProcessIdentity, ProcessInspector, SystemProcessInspector};

const DEFAULT_RUNTIME_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct SessionOutputChunk {
    pub internal_session_id: u64,
    pub session_handle: Arc<str>,
    pub invocation: InvocationContext,
    pub text: String,
}

pub trait SessionOutputObserver: Send + Sync {
    fn observe_output(&self, chunk: SessionOutputChunk);
}

#[derive(Debug, Clone)]
pub struct OwnedProcess {
    pub internal_session_id: u64,
    pub session_handle: Arc<str>,
    pub identity: ProcessIdentity,
    pub created_by: InvocationContext,
}

pub trait OwnedProcessObserver: Send + Sync {
    fn process_started(&self, process: &OwnedProcess) -> Result<()>;
    fn process_ended(&self, process: &OwnedProcess) -> Result<()>;
}

#[derive(Clone)]
pub struct SessionRuntimePolicy {
    execution: CommandExecutionProfile,
    process_inspector: Arc<dyn ProcessInspector>,
    output_observer: Arc<dyn SessionOutputObserver>,
    process_observer: Arc<dyn OwnedProcessObserver>,
    require_process_identity: bool,
    shutdown_grace: Duration,
}

impl SessionRuntimePolicy {
    pub fn sprite() -> Self {
        Self {
            execution: CommandExecutionProfile::Sprite,
            process_inspector: Arc::new(SystemProcessInspector),
            output_observer: Arc::new(NoopOutputObserver),
            process_observer: Arc::new(NoopOwnedProcessObserver),
            require_process_identity: false,
            shutdown_grace: DEFAULT_RUNTIME_SHUTDOWN_GRACE,
        }
    }

    pub fn local(
        shell: impl Into<PathBuf>,
        environment: Vec<(OsString, OsString)>,
    ) -> Result<Self> {
        let execution = LocalExecutionProfile::new(shell.into(), environment)?;
        Ok(Self {
            execution: CommandExecutionProfile::Local(execution),
            process_inspector: Arc::new(SystemProcessInspector),
            output_observer: Arc::new(NoopOutputObserver),
            process_observer: Arc::new(NoopOwnedProcessObserver),
            require_process_identity: true,
            shutdown_grace: DEFAULT_RUNTIME_SHUTDOWN_GRACE,
        })
    }

    pub fn with_process_inspector(mut self, inspector: Arc<dyn ProcessInspector>) -> Self {
        self.process_inspector = inspector;
        self
    }

    pub fn with_output_observer(mut self, observer: Arc<dyn SessionOutputObserver>) -> Self {
        self.output_observer = observer;
        self
    }

    pub fn with_process_observer(mut self, observer: Arc<dyn OwnedProcessObserver>) -> Self {
        self.process_observer = observer;
        self
    }

    pub fn with_shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    pub(crate) fn command(&self, cmd: &str, config: &Config) -> Command {
        self.execution.command(cmd, config)
    }

    pub(crate) fn process_inspector(&self) -> &Arc<dyn ProcessInspector> {
        &self.process_inspector
    }

    pub(crate) fn output_observer(&self) -> Arc<dyn SessionOutputObserver> {
        self.output_observer.clone()
    }

    pub(crate) fn process_observer(&self) -> Arc<dyn OwnedProcessObserver> {
        self.process_observer.clone()
    }

    pub(crate) fn require_process_identity(&self) -> bool {
        self.require_process_identity
    }

    pub(crate) fn shutdown_grace(&self) -> Duration {
        self.shutdown_grace
    }
}

#[derive(Clone)]
enum CommandExecutionProfile {
    Sprite,
    Local(LocalExecutionProfile),
}

impl CommandExecutionProfile {
    fn command(&self, cmd: &str, config: &Config) -> Command {
        let mut command = match self {
            Self::Sprite => {
                let mut command = Command::new("/bin/bash");
                command.arg("-lc").arg(cmd);
                if !config.agent_home.trim().is_empty() {
                    command.env("HOME", &config.agent_home);
                }
                command.env("USER", &config.agent_user);
                command.env("LOGNAME", &config.agent_user);
                command
            }
            Self::Local(local) => {
                let mut command = Command::new(&local.shell);
                command.arg("-c").arg(cmd).env_clear();
                for (key, value) in local.environment.iter() {
                    command.env(key, value);
                }
                command
            }
        };

        // Keep inherited Local developer-tool fidelity while ensuring commands
        // remain noninteractive, exactly as the existing Sprite runner does.
        command.env("PAGER", "cat");
        command.env("GIT_PAGER", "cat");
        command.env("LESS", "FRX");
        command.env("MANPAGER", "cat");
        command.env("SYSTEMD_PAGER", "cat");
        command
    }
}

#[derive(Clone)]
struct LocalExecutionProfile {
    shell: PathBuf,
    environment: Arc<[(OsString, OsString)]>,
}

impl LocalExecutionProfile {
    fn new(shell: PathBuf, environment: Vec<(OsString, OsString)>) -> Result<Self> {
        if !shell.is_absolute() {
            bail!(
                "Local command shell must be an absolute path: {}",
                shell.display()
            );
        }
        if shell.as_os_str().is_empty() {
            bail!("Local command shell must not be empty");
        }
        for required in ["HOME", "PATH"] {
            if !contains_nonempty_environment_key(&environment, required) {
                bail!("captured Local developer environment is missing `{required}`");
            }
        }
        Ok(Self {
            shell,
            environment: environment.into(),
        })
    }
}

fn contains_nonempty_environment_key(environment: &[(OsString, OsString)], key: &str) -> bool {
    environment
        .iter()
        .any(|(candidate, value)| candidate == OsStr::new(key) && !value.is_empty())
}

struct NoopOutputObserver;

impl SessionOutputObserver for NoopOutputObserver {
    fn observe_output(&self, _chunk: SessionOutputChunk) {}
}

struct NoopOwnedProcessObserver;

impl OwnedProcessObserver for NoopOwnedProcessObserver {
    fn process_started(&self, _process: &OwnedProcess) -> Result<()> {
        Ok(())
    }

    fn process_ended(&self, _process: &OwnedProcess) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::SessionRuntimePolicy;

    fn local_env() -> Vec<(OsString, OsString)> {
        [
            ("HOME", "/Users/test"),
            ("USER", "test"),
            ("LOGNAME", "test"),
            ("PATH", "/opt/homebrew/bin:/usr/bin:/bin"),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
    }

    #[test]
    fn local_profile_requires_absolute_shell_and_core_captured_environment() {
        assert!(SessionRuntimePolicy::local("zsh", local_env()).is_err());
        for missing in ["HOME", "PATH"] {
            let environment = local_env()
                .into_iter()
                .filter(|(key, _)| key != missing)
                .collect();
            assert!(SessionRuntimePolicy::local("/bin/zsh", environment).is_err());
        }
        SessionRuntimePolicy::local("/bin/zsh", local_env()).unwrap();
    }
}
