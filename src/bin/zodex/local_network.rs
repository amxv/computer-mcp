use super::*;

pub(super) const LOCAL_NETWORK_POLICY_VERSION: u32 = 1;
pub(super) const LOCAL_NETWORK_NAMESPACE: &str = "zodex-agent";
pub(super) const LOCAL_NETWORK_ROOT_INTERFACE: &str = "zdx-root0";
pub(super) const LOCAL_NETWORK_AGENT_INTERFACE: &str = "zdx-agent0";
pub(super) const LOCAL_NETWORK_ROOT_ADDRESS: &str = "100.127.255.1/30";
pub(super) const LOCAL_NETWORK_AGENT_ADDRESS: &str = "100.127.255.2/30";
pub(super) const LOCAL_NETWORK_AGENT_IPV4: &str = "100.127.255.2";
pub(super) const LOCAL_NETWORK_ROOT_GATEWAY: &str = "100.127.255.1";
pub(super) const LOCAL_NETWORK_SERVICE_NAME: &str = "zodex-local-network.service";
pub(super) const LOCAL_NETWORK_SCRIPT_PATH: &str = "/usr/local/libexec/zodex-local-network";
pub(super) const LOCAL_NETWORK_UNIT_PATH: &str = "/etc/systemd/system/zodex-local-network.service";

const LOCAL_NETWORK_SCRIPT_TEMPLATE: &str = include_str!("local_agent_network.sh");
const LOCAL_NETWORK_UNIT: &str = include_str!("local_agent_network.service");

// Deliberately broader than the IANA non-global rows in a few protocol-assignment blocks.
// This is the reviewed Local V1 definition of destinations that model traffic may not reach.
pub(super) const LOCAL_NON_PUBLIC_IPV4_CIDRS: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.0.2.0/24",
    "192.31.196.0/24",
    "192.52.193.0/24",
    "192.88.99.0/24",
    "192.168.0.0/16",
    "192.175.48.0/24",
    "198.18.0.0/15",
    "198.51.100.0/24",
    "203.0.113.0/24",
    "224.0.0.0/4",
    "240.0.0.0/4",
];

pub(super) const LOCAL_DEFAULT_PUBLIC_DNS: &[&str] = &["1.1.1.1", "8.8.8.8"];

pub(super) fn expected_local_network() -> LocalNetworkExpectation {
    LocalNetworkExpectation {
        policy_version: LOCAL_NETWORK_POLICY_VERSION,
        namespace: LOCAL_NETWORK_NAMESPACE.to_string(),
        root_interface: LOCAL_NETWORK_ROOT_INTERFACE.to_string(),
        agent_interface: LOCAL_NETWORK_AGENT_INTERFACE.to_string(),
    }
}

pub(super) fn local_network_expectation_matches(expected: &LocalNetworkExpectation) -> bool {
    expected == &expected_local_network()
}

pub(super) fn build_local_network_reconcile_script() -> String {
    let cidrs_space = LOCAL_NON_PUBLIC_IPV4_CIDRS.join(" ");
    let cidrs_nft = LOCAL_NON_PUBLIC_IPV4_CIDRS.join(", ");
    let default_dns = LOCAL_DEFAULT_PUBLIC_DNS.join(" ");
    LOCAL_NETWORK_SCRIPT_TEMPLATE
        .replace("@NAMESPACE@", LOCAL_NETWORK_NAMESPACE)
        .replace("@ROOT_INTERFACE@", LOCAL_NETWORK_ROOT_INTERFACE)
        .replace("@AGENT_INTERFACE@", LOCAL_NETWORK_AGENT_INTERFACE)
        .replace("@ROOT_ADDRESS@", LOCAL_NETWORK_ROOT_ADDRESS)
        .replace("@AGENT_ADDRESS@", LOCAL_NETWORK_AGENT_ADDRESS)
        .replace("@ROOT_GATEWAY@", LOCAL_NETWORK_ROOT_GATEWAY)
        .replace("@AGENT_IPV4@", LOCAL_NETWORK_AGENT_IPV4)
        .replace(
            "@POLICY_VERSION@",
            &LOCAL_NETWORK_POLICY_VERSION.to_string(),
        )
        .replace("@NON_PUBLIC_CIDRS_SPACE@", &cidrs_space)
        .replace("@NON_PUBLIC_CIDRS_NFT@", &cidrs_nft)
        .replace("@DEFAULT_DNS_SPACE@", &default_dns)
}

pub(super) fn local_network_unit() -> &'static str {
    LOCAL_NETWORK_UNIT
}

pub(super) fn local_agent_network_exec(command: &[String]) -> Vec<String> {
    let mut args = vec![
        "/usr/sbin/ip".to_string(),
        "netns".to_string(),
        "exec".to_string(),
        LOCAL_NETWORK_NAMESPACE.to_string(),
        "/usr/sbin/runuser".to_string(),
        "-u".to_string(),
        ZODEX_AGENT_USER.to_string(),
        "--".to_string(),
        "/usr/bin/env".to_string(),
        format!("HOME={ZODEX_AGENT_HOME}"),
    ];
    args.extend(command.iter().cloned());
    args
}

pub(super) fn local_root_network_verify_command() -> Vec<String> {
    vec![LOCAL_NETWORK_SCRIPT_PATH.to_string(), "verify".to_string()]
}
