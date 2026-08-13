    #[test]
    fn local_network_policy_is_versioned_interface_scoped_and_public_ipv4_only() {
        use super::local_network::{
            LOCAL_DEFAULT_PUBLIC_DNS, LOCAL_NETWORK_AGENT_INTERFACE, LOCAL_NETWORK_ROOT_INTERFACE,
            LOCAL_NON_PUBLIC_IPV4_CIDRS, build_local_network_reconcile_script,
        };

        let script = build_local_network_reconcile_script();
        for placeholder in [
            "@NAMESPACE@",
            "@ROOT_INTERFACE@",
            "@AGENT_INTERFACE@",
            "@NON_PUBLIC_CIDRS_SPACE@",
        ] {
            assert!(!script.contains(placeholder));
        }
        assert!(script.contains(&format!("POLICY_VERSION=\"{LOCAL_NETWORK_POLICY_VERSION}\"")));
        assert!(script.contains(&format!("ROOT_IF=\"{LOCAL_NETWORK_ROOT_INTERFACE}\"")));
        assert!(script.contains(&format!("AGENT_IF=\"{LOCAL_NETWORK_AGENT_INTERFACE}\"")));
        for cidr in LOCAL_NON_PUBLIC_IPV4_CIDRS {
            assert!(script.contains(cidr), "missing denied destination {cidr}");
        }
        for resolver in LOCAL_DEFAULT_PUBLIC_DNS {
            assert!(script.contains(resolver), "missing public DNS fallback {resolver}");
        }
        assert!(script.contains("input iifname \"${ROOT_IF}\" drop"));
        assert!(script.contains("forward iifname \"${ROOT_IF}\" ip daddr @non_public_v4 drop"));
        assert!(script.contains("forward iifname \"${ROOT_IF}\" meta nfproto ipv4 accept"));
        assert!(script.contains("forward iifname \"${ROOT_IF}\" drop"));
        assert!(script.contains("ct state established,related accept"));
        assert!(script.contains("net.ipv6.conf.all.disable_ipv6=1"));
        assert!(script.contains("mount -o remount,rw /proc/sys"));
        assert!(script.contains("trap 'mount -o remount,ro /proc/sys' EXIT"));
        assert!(script.contains("sysctl -q -w net.ipv4.ip_forward=1"));
        assert!(script.contains("route show default | awk '{$1=$1; print}'"));
        assert!(script.contains("DNS_DIR=\"/etc/netns/${NS}\""));
        assert!(script.contains("DNS_FILE=\"${DNS_DIR}/resolv.conf\""));
        assert!(script.contains("nft delete table inet \"$FILTER_TABLE\""));
        assert!(script.contains("if nft list table inet \"$FILTER_TABLE\""));
        assert!(script.contains("if nft list table ip \"$NAT_TABLE\""));
        assert!(!script.contains("flush ruleset"));
        assert!(script.contains("cmp -s <(nft list table inet \"$FILTER_TABLE\")"));
        assert!(script.contains("cmp -s <(nft list table ip \"$NAT_TABLE\")"));
        assert!(!script.contains("filter.current"));
        assert!(!script.contains("nat.current"));
    }

    #[test]
    fn local_service_assets_share_one_network_namespace_and_drop_capabilities() {
        use super::local_network::{local_network_unit, LOCAL_NETWORK_SCRIPT_PATH};

        let daemon = include_str!("../local_zodexd.service");
        let publisher = include_str!("../local_zodex_prd.service");
        for unit in [daemon, publisher] {
            assert!(unit.contains("Requires=zodex-local-network.service"));
            assert!(unit.contains("NetworkNamespacePath=/run/netns/zodex-agent"));
            assert!(unit.contains("BindReadOnlyPaths=/etc/netns/zodex-agent/resolv.conf:/etc/resolv.conf"));
            assert!(unit.contains("CapabilityBoundingSet=\n"));
            assert!(unit.contains("AmbientCapabilities=\n"));
            assert!(unit.contains("NoNewPrivileges=yes"));
        }
        assert!(daemon.contains("User=zodex-agent"));
        assert!(publisher.contains("User=zodex-publisher"));
        let network = local_network_unit();
        assert!(network.contains("Before=zodex-prd.service zodexd.service"));
        assert!(network.contains(&format!("ExecStart={LOCAL_NETWORK_SCRIPT_PATH} reconcile")));
    }

    #[test]
    fn local_guest_setup_stops_model_services_before_network_reconcile() {
        let sources = LocalSetupSources {
            repo: "amxv/zodex".to_string(),
            reader_app_id: 1,
            reader_pem_path: "/tmp/reader.pem".to_string(),
            reader_installation_id: 2,
            publisher_app_id: 3,
            publisher_pem_path: "/tmp/publisher.pem".to_string(),
            publisher_installation_id: 4,
            default_base: "main".to_string(),
            tunnel_id: Some("tunnel_0123456789abcdef0123456789abcdef".to_string()),
            tunnel_runtime_key_path: Some("/tmp/tunnel-runtime-key".to_string()),
        };
        let script = build_local_guest_setup_script(&sources).expect("setup script");
        let preinstall_repair = script.find("if [[ -f \"$CFG\" ]]").unwrap();
        let installer = script.find("TMP_INSTALLER=\"$(mktemp)\"").unwrap();
        assert!(preinstall_repair < installer);
        let stop = script
            .find("systemctl stop zodex-tunnel.service zodexd.service zodex-prd.service")
            .unwrap();
        let network = script.find("systemctl restart zodex-local-network.service").unwrap();
        let start = script.find("systemctl enable --now zodex-prd.service zodexd.service").unwrap();
        assert!(stop < network && network < start);
        assert!(script.starts_with("#!/usr/bin/env bash\nset -euo pipefail"));
    }

    #[test]
    fn local_agent_network_exec_preserves_tokens_and_stays_unprivileged() {
        use super::local_network::local_agent_network_exec;

        let command = vec!["printf".to_string(), "%s\\n".to_string(), "a b".to_string()];
        let args = local_agent_network_exec(&command);
        assert_eq!(
            &args[..9],
            [
                "/usr/sbin/ip",
                "netns",
                "exec",
                "zodex-agent",
                "/usr/sbin/runuser",
                "-u",
                "zodex-agent",
                "--",
                "/usr/bin/env",
            ]
        );
        assert_eq!(args[9], "HOME=/home/zodex-agent");
        assert_eq!(&args[10..], command.as_slice());
    }

    #[test]
    fn ready_local_state_requires_current_network_identity() {
        let expected = expected_local_network();
        assert_eq!(expected.policy_version, LOCAL_NETWORK_POLICY_VERSION);
        assert_eq!(expected.namespace, LOCAL_NETWORK_NAMESPACE);

        let stale = LocalNetworkExpectation {
            policy_version: LOCAL_NETWORK_POLICY_VERSION + 1,
            ..expected.clone()
        };
        assert!(super::local_network::local_network_expectation_matches(&expected));
        assert!(!super::local_network::local_network_expectation_matches(&stale));

        let temp = tempfile::tempdir().expect("temp dir");
        let target_path = local_target_state_path_from_home(temp.path());
        fs::create_dir_all(target_path.parent().expect("target parent")).expect("state dir");
        fs::write(
            &target_path,
            r#"{"version":1,"machine_id":"zodex-local","setup_state":"ready","setup_sources":{"repo":"amxv/zodex","reader_app_id":1,"reader_pem_path":"/tmp/r","reader_installation_id":2,"publisher_app_id":3,"publisher_pem_path":"/tmp/p","publisher_installation_id":4,"default_base":"main"}}"#,
        )
        .expect("write ready target without network identity");
        let error = load_local_target_record(&target_path)
            .expect_err("ready target without network identity must fail closed")
            .to_string();
        assert!(error.contains("missing network policy identity"));
    }

    #[test]
    fn browser_open_attempts_include_platform_fallback() {
        let attempts = browser_open_attempts("https://github.com/login/device");
        assert!(!attempts.is_empty());

        if cfg!(target_os = "macos") {
            assert_eq!(attempts[0].0, "open");
        } else if cfg!(target_os = "windows") {
            assert_eq!(attempts[0].0, "cmd");
        } else {
            assert!(attempts.iter().any(|(program, _)| *program == "xdg-open"));
        }
    }
