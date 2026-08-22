#[cfg(target_os = "macos")]
mod assets;
#[cfg(target_os = "macos")]
mod bridge;
#[cfg(target_os = "macos")]
mod discovery;
mod launch;
#[cfg(target_os = "macos")]
mod notifier;
#[cfg(target_os = "macos")]
mod prefs;
#[cfg(target_os = "macos")]
mod server;

#[cfg(all(test, target_os = "macos"))]
mod server_tests;

pub use launch::{run_local_liveboard, run_local_liveboard_without_open};

#[cfg(target_os = "macos")]
pub(crate) use discovery::{
    LocalLiveboardDiscovery, remove_liveboard_discovery, write_liveboard_discovery,
};
#[cfg(target_os = "macos")]
pub(crate) use notifier::LiveboardLinkNotifier;
#[cfg(target_os = "macos")]
pub(crate) use server::{LocalLiveboardHost, start_liveboard_host};
