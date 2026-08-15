mod file_store;
mod query;
mod schema;
mod store;
mod worker;

#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod shutdown_tests;
#[cfg(test)]
mod tests;

pub use query::{
    HistoryAgentSummary, HistoryAgentWorkdir, HistoryFileEvidence, HistoryFormat,
    HistoryInvocation, HistoryQuery, HistoryStoreStatus, LocalHistoryReader,
};
pub use schema::HISTORY_SCHEMA_VERSION;
pub use worker::{LocalHistoryRuntime, LocalHistoryRuntimeConfig};

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub fn clear_local_history(path: &Path) -> Result<()> {
    for candidate in history_store_paths(path) {
        if candidate.exists() {
            fs::remove_file(&candidate).with_context(|| {
                format!(
                    "failed to remove Local history store {}",
                    candidate.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn history_store_paths(path: &Path) -> [std::path::PathBuf; 3] {
    [
        path.to_path_buf(),
        std::path::PathBuf::from(format!("{}-wal", path.display())),
        std::path::PathBuf::from(format!("{}-shm", path.display())),
    ]
}
