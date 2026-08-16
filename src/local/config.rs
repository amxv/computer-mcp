use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use super::{HumanDuration, StorageSize};

const DEFAULT_HISTORY_MAX_AGE_SECONDS: u64 = 60 * 24 * 60 * 60;
const DEFAULT_HISTORY_MAX_SIZE_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LocalConfig {
    pub tunnel: LocalTunnelConfig,
    pub history: LocalHistoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LocalTunnelConfig {
    pub id: Option<String>,
    pub client_path: Option<PathBuf>,
    pub release: Option<ManagedTunnelClientRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ManagedTunnelClientRelease {
    pub version: String,
    pub asset_name: String,
    pub archive_sha256: String,
    pub binary_sha256: String,
    pub cloudflared_sha256: String,
    pub cloudflared_manifest_sha256: String,
    pub source_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct LocalHistoryConfig {
    pub max_age: HumanDuration,
    pub max_size: StorageSize,
}

impl Default for LocalHistoryConfig {
    fn default() -> Self {
        Self {
            max_age: HumanDuration::from_seconds(DEFAULT_HISTORY_MAX_AGE_SECONDS),
            max_size: StorageSize::from_bytes(DEFAULT_HISTORY_MAX_SIZE_BYTES),
        }
    }
}

impl LocalConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read Local config at {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse Local config at {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .context("Local config path must have a parent directory")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create Local config directory {}",
                parent.display()
            )
        })?;
        let raw = toml::to_string_pretty(self).context("failed to serialize Local config")?;
        let mut temp = NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "failed to create temporary Local config in {}",
                parent.display()
            )
        })?;
        use std::io::Write as _;
        temp.write_all(raw.as_bytes())
            .context("failed to write temporary Local config")?;
        temp.as_file()
            .sync_all()
            .context("failed to sync temporary Local config")?;
        temp.persist(path)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to persist Local config at {}", path.display()))?;
        Ok(())
    }

    pub fn is_provider_configured(&self) -> bool {
        self.tunnel
            .id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && self.tunnel.client_path.is_some()
            && self.tunnel.release.is_some()
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "history.max-age" => self.history.max_age = value.parse()?,
            "history.max-size" => self.history.max_size = value.parse()?,
            "tunnel.id" => {
                let value = value.trim();
                super::validate_tunnel_id(value)?;
                self.tunnel.id = Some(value.to_owned());
            }
            _ => bail!(
                "unknown Local config key `{key}`; supported keys: history.max-age, history.max-size, tunnel.id"
            ),
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<String> {
        match key {
            "history.max-age" => Ok(self.history.max_age.to_string()),
            "history.max-size" => Ok(self.history.max_size.to_string()),
            "tunnel.id" => Ok(self
                .tunnel
                .id
                .clone()
                .unwrap_or_else(|| "unset".to_string())),
            "tunnel.client-path" => Ok(self
                .tunnel
                .client_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unset".to_string())),
            _ => bail!(
                "unknown Local config key `{key}`; supported keys: history.max-age, history.max-size, tunnel.id, tunnel.client-path"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::LocalConfig;

    #[test]
    fn local_config_defaults_and_round_trip_are_human_readable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local.toml");
        let mut config = LocalConfig::default();
        assert_eq!(config.get("history.max-age").unwrap(), "60d");
        assert_eq!(config.get("history.max-size").unwrap(), "500mb");
        assert!(!config.is_provider_configured());

        config.set("history.max-age", "2d").unwrap();
        config.set("history.max-size", "1gb").unwrap();
        config
            .set("tunnel.id", "tunnel_0123456789abcdef0123456789abcdef")
            .unwrap();
        config.tunnel.client_path = Some(dir.path().join("tunnel-client"));
        config.tunnel.release = Some(super::ManagedTunnelClientRelease {
            version: "v0.0.11".to_string(),
            asset_name: "tunnel-client-v0.0.11-darwin-arm64.zip".to_string(),
            archive_sha256: "a".repeat(64),
            binary_sha256: "b".repeat(64),
            cloudflared_sha256: "c".repeat(64),
            cloudflared_manifest_sha256: "d".repeat(64),
            source_url: "https://example.invalid/archive.zip".to_string(),
        });
        config.save(&path).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("max_age = \"2d\""));
        assert!(raw.contains("max_size = \"1gb\""));
        assert!(!raw.contains("api_key"));
        assert!(!raw.contains("runtime_key"));
        assert!(config.is_provider_configured());

        let loaded = LocalConfig::load(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn local_config_rejects_invalid_values_and_unknown_fields() {
        let mut config = LocalConfig::default();
        assert!(config.set("history.max-age", "forever").is_err());
        assert!(config.set("history.max-size", "huge").is_err());
        assert!(config.set("secret.api-key", "do-not-store").is_err());

        let parsed = toml::from_str::<LocalConfig>(
            "[history]\nmax_age = \"60d\"\nmax_size = \"500mb\"\nsecret = \"nope\"\n",
        );
        assert!(parsed.is_err());
    }
}
