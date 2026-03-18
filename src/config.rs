use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/computer-mcp/config.toml";

const MIN_YIELD_MS: u64 = 50;
const MAX_YIELD_MS: u64 = 60_000;
const MIN_EXEC_TIMEOUT_MS: u64 = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub bind_host: String,
    pub bind_port: u16,
    pub api_key: String,
    pub tls_mode: String,
    pub tls_cert_path: String,
    pub tls_key_path: String,
    pub max_sessions: usize,
    pub default_exec_timeout_ms: u64,
    pub max_exec_timeout_ms: u64,
    pub default_exec_yield_time_ms: u64,
    pub default_write_yield_time_ms: u64,
    pub max_output_chars: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_host: "0.0.0.0".to_string(),
            bind_port: 443,
            api_key: "change-me".to_string(),
            tls_mode: "auto".to_string(),
            tls_cert_path: "/var/lib/computer-mcp/tls/cert.pem".to_string(),
            tls_key_path: "/var/lib/computer-mcp/tls/key.pem".to_string(),
            max_sessions: 64,
            default_exec_timeout_ms: 7_200_000,
            max_exec_timeout_ms: 7_200_000,
            default_exec_yield_time_ms: 10_000,
            default_write_yield_time_ms: 10_000,
            max_output_chars: 200_000,
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let parsed = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        Ok(parsed)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }

        let raw = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(path, raw)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
        Ok(())
    }

    pub fn clamp_exec_yield_ms(&self, requested: Option<u64>) -> u64 {
        let raw = requested.unwrap_or(self.default_exec_yield_time_ms);
        raw.clamp(MIN_YIELD_MS, MAX_YIELD_MS)
    }

    pub fn clamp_write_yield_ms(&self, requested: Option<u64>) -> u64 {
        let raw = requested.unwrap_or(self.default_write_yield_time_ms);
        raw.clamp(MIN_YIELD_MS, MAX_YIELD_MS)
    }

    pub fn clamp_exec_timeout_ms(&self, requested: Option<u64>) -> u64 {
        let raw = requested.unwrap_or(self.default_exec_timeout_ms);
        raw.clamp(MIN_EXEC_TIMEOUT_MS, self.max_exec_timeout_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn clamp_yields_and_timeout() {
        let cfg = Config::default();

        assert_eq!(cfg.clamp_exec_yield_ms(Some(1)), 50);
        assert_eq!(cfg.clamp_write_yield_ms(Some(100_000)), 60_000);
        assert_eq!(cfg.clamp_exec_timeout_ms(Some(1)), 1_000);
        assert_eq!(cfg.clamp_exec_timeout_ms(Some(9_000_000)), 7_200_000);
    }
}
