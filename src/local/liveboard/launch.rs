#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::{Result, bail};

use super::super::LocalPaths;
#[cfg(target_os = "macos")]
use super::server::start_liveboard_host;

#[cfg(target_os = "macos")]
pub(crate) trait BrowserLauncher: Send + Sync {
    fn open(&self, url: &str) -> Result<()>;
}

#[cfg(target_os = "macos")]
struct SystemBrowserLauncher;

#[cfg(target_os = "macos")]
impl BrowserLauncher for SystemBrowserLauncher {
    fn open(&self, url: &str) -> Result<()> {
        let status = Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .context("failed to launch the default browser with /usr/bin/open")?;
        if !status.success() {
            bail!("/usr/bin/open exited with status {status}");
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub async fn run_local_liveboard(paths: &LocalPaths) -> Result<()> {
    run_local_liveboard_with_launcher(paths, Some(&SystemBrowserLauncher)).await
}

#[cfg(not(target_os = "macos"))]
pub async fn run_local_liveboard(_paths: &LocalPaths) -> Result<()> {
    bail!("Zodex Local Liveboard is only available on macOS")
}

#[cfg(target_os = "macos")]
pub async fn run_local_liveboard_without_open(paths: &LocalPaths) -> Result<()> {
    run_local_liveboard_with_launcher(paths, None).await
}

#[cfg(not(target_os = "macos"))]
pub async fn run_local_liveboard_without_open(_paths: &LocalPaths) -> Result<()> {
    bail!("Zodex Local Liveboard is only available on macOS")
}

#[cfg(target_os = "macos")]
pub(crate) async fn run_local_liveboard_with_launcher(
    paths: &LocalPaths,
    launcher: Option<&dyn BrowserLauncher>,
) -> Result<()> {
    let host = start_liveboard_host(paths).await?;
    println!("Liveboard: {}", host.url());
    if let Some(launcher) = launcher
        && let Err(error) = launcher.open(host.url())
    {
        eprintln!(
            "warning: could not open the default browser automatically: {error:#}\nOpen {} manually.",
            host.url()
        );
    }
    tokio::signal::ctrl_c()
        .await
        .context("failed to wait for Liveboard Ctrl-C")?;
    host.shutdown().await
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::sync::Mutex;

    use anyhow::Result;

    use super::BrowserLauncher;

    struct RecordingLauncher {
        urls: Mutex<Vec<String>>,
        fail: bool,
    }

    impl BrowserLauncher for RecordingLauncher {
        fn open(&self, url: &str) -> Result<()> {
            self.urls.lock().unwrap().push(url.to_string());
            if self.fail {
                anyhow::bail!("browser unavailable")
            }
            Ok(())
        }
    }

    #[test]
    fn browser_launcher_abstraction_records_capability_url_and_can_fail_without_panicking() {
        let launcher = RecordingLauncher {
            urls: Mutex::new(Vec::new()),
            fail: true,
        };
        assert!(launcher.open("http://127.0.0.1:1234/capability/").is_err());
        assert_eq!(
            launcher.urls.lock().unwrap().as_slice(),
            ["http://127.0.0.1:1234/capability/"]
        );
    }
}
