use std::fs;
use std::io::Read as _;
use std::path::Path;

use super::{FileSnapshot, MAX_FILE_EVIDENCE_BYTES};

pub(super) fn capture_text_snapshot(path: &Path) -> FileSnapshot {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return FileSnapshot::Missing,
        Err(error) => {
            return FileSnapshot::Unavailable(format!("failed to inspect file: {error}"));
        }
    };
    if !metadata.is_file() {
        return FileSnapshot::Unavailable("path is not a regular file".to_string());
    }
    if metadata.len() > MAX_FILE_EVIDENCE_BYTES {
        return FileSnapshot::Unavailable(format!(
            "file exceeds {MAX_FILE_EVIDENCE_BYTES}-byte enrichment limit"
        ));
    }

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return FileSnapshot::Unavailable(format!("failed to open file: {error}")),
    };
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len().min(MAX_FILE_EVIDENCE_BYTES)).unwrap_or(0),
    );
    if let Err(error) = file
        .take(MAX_FILE_EVIDENCE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
    {
        return FileSnapshot::Unavailable(format!("failed to read file: {error}"));
    }
    if bytes.len() as u64 > MAX_FILE_EVIDENCE_BYTES {
        return FileSnapshot::Unavailable(format!(
            "file grew beyond {MAX_FILE_EVIDENCE_BYTES}-byte enrichment limit"
        ));
    }
    match String::from_utf8(bytes) {
        Ok(text) => FileSnapshot::Text(text),
        Err(_) => FileSnapshot::Unavailable("file is not UTF-8 text".to_string()),
    }
}
