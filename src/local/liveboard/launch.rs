use std::process::Command;

use anyhow::{Context, Result, bail};

use super::super::LocalPaths;
use super::server::start_liveboard_host;

pub(crate) trait BrowserLauncher: Send + Sync {
    fn open(&self, url: &str) -> Result<()>;
}

struct SystemBrowserLauncher;

impl BrowserLauncher for SystemBrowserLauncher {
    fn open(&self, url: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let status = Command::new("/usr/bin/open")
                .arg(url)
                .status()
                .context("failed to launch the default browser with /usr/bin/open")?;
            if !status.success() {
                bail!("/usr/bin/open exited with status {status}");
            }
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = url;
            bail!("Liveboard browser launch is only supported on macOS")
        }
    }
}

pub async fn run_local_liveboard(paths: &LocalPaths) -> Result<()> {
    run_local_liveboard_with_launcher(paths, &SystemBrowserLauncher).await
}

pub(crate) async fn run_local_liveboard_with_launcher(
    paths: &LocalPaths,
    launcher: &dyn BrowserLauncher,
) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (paths, launcher);
        bail!("Zodex Local Liveboard is only available on macOS")
    }

    #[cfg(target_os = "macos")]
    {
        let host = start_liveboard_host(paths).await?;
        println!("Liveboard: {}", host.url());
        if let Err(error) = launcher.open(host.url()) {
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
}

#[cfg(test)]
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
