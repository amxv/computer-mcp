use anyhow::{Result, bail};

pub(super) struct EmbeddedAsset {
    pub(super) path: &'static str,
    pub(super) bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/liveboard_assets.rs"));

pub(super) fn ensure_available() -> Result<()> {
    if EMBEDDED_AVAILABLE {
        Ok(())
    } else {
        bail!("{EMBEDDED_UNAVAILABLE_REASON}")
    }
}

pub(super) fn find(path: &str) -> Option<&'static EmbeddedAsset> {
    EMBEDDED_ASSETS.iter().find(|asset| asset.path == path)
}

#[cfg(test)]
pub(super) fn all() -> &'static [EmbeddedAsset] {
    EMBEDDED_ASSETS
}

pub(super) fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

pub(super) fn immutable(path: &str) -> bool {
    let Some(file_name) = path.strip_prefix("assets/") else {
        return false;
    };
    let Some((stem, _extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    let Some((_name, fingerprint)) = stem.rsplit_once('-') else {
        return false;
    };
    fingerprint.len() >= 8
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{all, content_type, ensure_available, find, immutable};

    #[test]
    fn embedded_asset_table_has_safe_relative_paths_when_available() {
        if let Err(error) = ensure_available() {
            let message = error.to_string();
            assert!(message.contains("Liveboard assets were not embedded"));
            assert!(message.contains("web/liveboard"));
            assert!(message.contains("bun run build"));
            return;
        }
        assert!(find("index.html").is_some());
        assert!(all().iter().all(|asset| {
            !asset.path.starts_with('/') && !asset.path.contains("..") && !asset.path.contains('\\')
        }));
        assert_eq!(
            content_type("assets/main.js"),
            "text/javascript; charset=utf-8"
        );
        assert!(immutable("assets/main-deadbeef.js"));
        assert!(!immutable("assets/main.js"));
        assert!(!immutable("index.html"));
    }
}
