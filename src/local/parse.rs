use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HumanDuration(u64);

impl HumanDuration {
    pub const fn from_seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn seconds(self) -> u64 {
        self.0
    }
}

impl fmt::Display for HumanDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.0;
        if seconds.is_multiple_of(86_400) {
            write!(formatter, "{}d", seconds / 86_400)
        } else if seconds.is_multiple_of(3_600) {
            write!(formatter, "{}h", seconds / 3_600)
        } else if seconds.is_multiple_of(60) {
            write!(formatter, "{}min", seconds / 60)
        } else {
            write!(formatter, "{seconds}s")
        }
    }
}

impl FromStr for HumanDuration {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self> {
        parse_human_duration(raw).map(Self)
    }
}

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageSize(u64);

impl StorageSize {
    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> u64 {
        self.0
    }
}

impl fmt::Display for StorageSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0;
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * KIB;
        const GIB: u64 = 1024 * MIB;
        if bytes.is_multiple_of(GIB) {
            write!(formatter, "{}gb", bytes / GIB)
        } else if bytes.is_multiple_of(MIB) {
            write!(formatter, "{}mb", bytes / MIB)
        } else if bytes.is_multiple_of(KIB) {
            write!(formatter, "{}kb", bytes / KIB)
        } else {
            write!(formatter, "{bytes}b")
        }
    }
}

impl FromStr for StorageSize {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self> {
        parse_storage_size(raw).map(Self)
    }
}

impl Serialize for StorageSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for StorageSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

pub fn parse_human_duration(raw: &str) -> Result<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("duration must not be empty");
    }

    let split = trimmed
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (amount_raw, unit_raw) = trimmed.split_at(split);
    if amount_raw.is_empty() {
        bail!("duration `{raw}` must start with a positive integer");
    }
    let amount = amount_raw
        .parse::<u64>()
        .map_err(|_| anyhow!("failed to parse duration `{raw}`"))?;
    if amount == 0 {
        bail!("duration must be greater than zero");
    }

    let multiplier = match unit_raw.to_ascii_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600,
        "d" | "day" | "days" => 86_400,
        "" => bail!("duration `{raw}` is missing a unit; use s, min, h, or d"),
        unit => bail!("unsupported duration unit `{unit}`; use s, min, h, or d"),
    };

    amount
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("duration `{raw}` is too large"))
}

pub fn parse_storage_size(raw: &str) -> Result<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("size must not be empty");
    }

    let split = trimmed
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (amount_raw, unit_raw) = trimmed.split_at(split);
    if amount_raw.is_empty() {
        bail!("size `{raw}` must start with a positive integer");
    }
    let amount = amount_raw
        .parse::<u64>()
        .map_err(|_| anyhow!("failed to parse size `{raw}`"))?;
    if amount == 0 {
        bail!("size must be greater than zero");
    }

    let multiplier = match unit_raw.to_ascii_lowercase().as_str() {
        "b" | "bytes" => 1,
        "kb" | "kib" => 1024,
        "mb" | "mib" => 1024 * 1024,
        "gb" | "gib" => 1024 * 1024 * 1024,
        "" => bail!("size `{raw}` is missing a unit; use b, kb, mb, or gb"),
        unit => bail!("unsupported size unit `{unit}`; use b, kb, mb, or gb"),
    };

    amount
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("size `{raw}` is too large"))
}

#[cfg(test)]
mod tests {
    use super::{HumanDuration, StorageSize, parse_human_duration, parse_storage_size};

    #[test]
    fn duration_parser_accepts_local_contract_units() {
        assert_eq!(parse_human_duration("30min").unwrap(), 1_800);
        assert_eq!(parse_human_duration("4h").unwrap(), 14_400);
        assert_eq!(parse_human_duration("2d").unwrap(), 172_800);
        assert_eq!(parse_human_duration("30m").unwrap(), 1_800);
        assert_eq!(HumanDuration::from_seconds(5_184_000).to_string(), "60d");
    }

    #[test]
    fn duration_parser_rejects_ambiguous_or_invalid_values() {
        for raw in ["", "0h", "30", "2w", "-1h", "1.5h"] {
            assert!(parse_human_duration(raw).is_err(), "{raw} should fail");
        }
    }

    #[test]
    fn size_parser_accepts_local_contract_units() {
        assert_eq!(parse_storage_size("500mb").unwrap(), 500 * 1024 * 1024);
        assert_eq!(parse_storage_size("1GiB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(
            StorageSize::from_bytes(500 * 1024 * 1024).to_string(),
            "500mb"
        );
    }

    #[test]
    fn size_parser_rejects_ambiguous_or_invalid_values() {
        for raw in ["", "0mb", "500", "2tb", "-1mb", "1.5gb"] {
            assert!(parse_storage_size(raw).is_err(), "{raw} should fail");
        }
    }
}
