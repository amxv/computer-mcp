include!("prelude.rs");
include!("dispatch.rs");
include!("credentials.rs");
include!("local_state.rs");
include!("local_provider.rs");
mod local_lifecycle;
mod local_network;
mod local_setup;
mod local_tunnel;
include!("sprite_proxy.rs");
include!("github_device.rs");
include!("github_mode.rs");
include!("lifecycle.rs");
include!("process.rs");
include!("status.rs");
include!("system_tls.rs");

#[cfg(test)]
mod tests {
    include!("tests/part1.rs");
    include!("tests/part2.rs");
    include!("tests/part3.rs");
    include!("tests/part4.rs");
}
