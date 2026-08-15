mod config;
mod parse;
mod paths;
mod secret;
mod setup;
mod status;
mod tunnel_provider;
mod tunnel_release;

#[cfg(test)]
mod setup_tests;

pub use config::{LocalConfig, LocalHistoryConfig, LocalTunnelConfig, ManagedTunnelClientRelease};
pub use parse::{HumanDuration, StorageSize, parse_human_duration, parse_storage_size};
pub use paths::LocalPaths;
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
