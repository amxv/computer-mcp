pub mod apply_patch;
pub mod client;
pub mod config;
pub mod http_api;
pub mod protocol;
pub mod publisher;
pub mod redaction;
pub mod server;
pub mod service;
pub mod session;

pub fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
