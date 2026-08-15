mod config;
mod file_evidence;
mod history;
mod observability;
mod parse;
mod paths;
mod presentation;
mod process_registry;
mod runtime;
mod secret;
mod setup;
mod status;
mod tunnel_provider;
mod tunnel_release;

#[cfg(test)]
mod setup_tests;

pub use config::{LocalConfig, LocalHistoryConfig, LocalTunnelConfig, ManagedTunnelClientRelease};
pub use history::{
    HISTORY_SCHEMA_VERSION, HistoryAgentSummary, HistoryAgentWorkdir, HistoryFileEvidence,
    HistoryFormat, HistoryInvocation, HistoryQuery, HistoryStoreStatus, LocalHistoryReader,
    LocalHistoryRuntime, LocalHistoryRuntimeConfig, clear_local_history,
};
pub use observability::{
    LOCAL_OBSERVABILITY_API_VERSION, LocalObservabilityServer, start_local_observability_server,
};
pub use parse::{HumanDuration, StorageSize, parse_human_duration, parse_storage_size};
pub use paths::LocalPaths;
pub use presentation::{
    PRESENTATION_SCHEMA_VERSION, PresentationAgent, PresentationDiffLine, PresentationDocument,
    PresentationEvidence, PresentationFileChange, PresentationFileOperation, PresentationKind,
    PresentationPollSummary, PresentationRecord, PresentationWorkdir, PresentationWriteMode,
    build_presentation, render_presentation,
};
pub use process_registry::{
    LOCAL_PROCESS_REGISTRY_SCHEMA_VERSION, LocalOwnedProcessRecord, LocalOwnedProcessRegistry,
    LocalProcessRegistryDocument, StaleProcessCleanupReport, signal_matching_stale_processes,
};
pub use runtime::{LocalHostRuntime, LocalHostRuntimeOptions, start_local_host_runtime};
#[cfg(target_os = "macos")]
pub use secret::MacKeychainRuntimeKeyStore;
pub use secret::{RuntimeKey, RuntimeKeyStore};
pub use setup::{
    LocalSetupRequest, LocalSetupResult, LocalSetupService, ensure_observability_bearer,
};
pub use status::{
    LOCAL_DISCOVERY_SCHEMA_VERSION, LOCAL_RUNTIME_STATE_SCHEMA_VERSION,
    LOCAL_STATUS_SCHEMA_VERSION, LocalObservabilityDiscovery, LocalRuntimeDiscovery,
    LocalRuntimeLifecycle, LocalRuntimeState, LocalStatusDocument, LocalStatusState,
    ensure_offline_mutation,
};
#[cfg(target_os = "macos")]
pub use tunnel_provider::MacDittoArchiveExtractor;
pub use tunnel_provider::{
    ArchiveExtractor, ProcessTunnelMetadataValidator, TunnelMetadataValidator,
};
pub use tunnel_release::{
    OfficialTunnelReleaseClient, ResolvedTunnelRelease, TunnelArchitecture, sha256_hex,
    validate_tunnel_id,
};
