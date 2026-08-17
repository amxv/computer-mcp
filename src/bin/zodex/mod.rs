include!("prelude.rs");
include!("local.rs");
include!("dispatch.rs");
include!("credentials.rs");
include!("sprite_services.rs");
include!("proxy_worker.rs");
include!("cloudflare_agent.rs");
include!("sprite_connect.rs");
include!("sprite_proxy.rs");
include!("sprite_health.rs");
include!("sprite_setup.rs");
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
    include!("tests/part5.rs");
}
