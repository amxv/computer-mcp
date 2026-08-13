    use super::local_lifecycle::{
        LocalLeaseView, LocalLeaseWorkerDecision, LocalLifecycleRuntime,
        build_local_lease_launchd_plist, local_lease_view, local_lease_worker_decision,
        parse_local_access_ttl, revoke_local_access_with_runtime, start_local_access_with_runtime,
    };
    use super::local_tunnel::{
        LOCAL_TUNNEL_ARCHIVE_SHA256, LOCAL_TUNNEL_ARCHIVE_URL, LOCAL_TUNNEL_VERSION,
        build_local_tunnel_install_fragment, validate_local_tunnel_id,
    };
    use anyhow::{Result as AnyResult, bail};

    const TEST_TUNNEL_ID: &str = "tunnel_0123456789abcdef0123456789abcdef";

    fn phase_three_target() -> LocalTargetRecord {
        LocalTargetRecord {
            version: 1,
            machine_id: LOCAL_MACHINE_NAME.to_string(),
            setup_state: LocalSetupState::Ready,
            image_reference: Some("local/zodex-machine:1".to_string()),
            requested_cpus: None,
            requested_memory: None,
            network: Some(expected_local_network()),
            setup_sources: Some(LocalSetupSources {
                repo: "amxv/zodex".to_string(),
                reader_app_id: 1,
                reader_pem_path: "/tmp/reader.pem".to_string(),
                reader_installation_id: 2,
                publisher_app_id: 3,
                publisher_pem_path: "/tmp/publisher.pem".to_string(),
                publisher_installation_id: 4,
                default_base: "main".to_string(),
                tunnel_id: Some(TEST_TUNNEL_ID.to_string()),
                tunnel_runtime_key_path: Some("/tmp/tunnel-runtime-key".to_string()),
            }),
        }
    }

    #[derive(Default)]
    struct FakeLocalLifecycleRuntime {
        now: u64,
        events: Vec<String>,
        fail_prepare: bool,
        fail_start_tunnel: bool,
        fail_stop_tunnel: bool,
        fail_stop_machine: bool,
        fail_arm: bool,
    }

    impl LocalLifecycleRuntime for FakeLocalLifecycleRuntime {
        fn now_epoch_seconds(&mut self) -> AnyResult<u64> {
            Ok(self.now)
        }

        fn prepare_runtime(&mut self, _target: &LocalTargetRecord) -> AnyResult<()> {
            self.events.push("prepare".to_string());
            if self.fail_prepare {
                bail!("injected prepare failure");
            }
            Ok(())
        }

        fn start_tunnel(&mut self) -> AnyResult<()> {
            self.events.push("start_tunnel".to_string());
            if self.fail_start_tunnel {
                bail!("injected tunnel start failure");
            }
            Ok(())
        }

        fn stop_tunnel(&mut self) -> AnyResult<()> {
            self.events.push("stop_tunnel".to_string());
            if self.fail_stop_tunnel {
                bail!("injected tunnel stop failure");
            }
            Ok(())
        }

        fn stop_machine(&mut self) -> AnyResult<()> {
            self.events.push("stop_machine".to_string());
            if self.fail_stop_machine {
                bail!("injected machine stop failure");
            }
            Ok(())
        }

        fn arm_supervisor(&mut self, lease: &LocalAccessLease) -> AnyResult<()> {
            self.events.push(format!("arm:{}", lease.generation));
            if self.fail_arm {
                bail!("injected supervisor arm failure");
            }
            Ok(())
        }

        fn disarm_supervisor(&mut self) -> AnyResult<()> {
            self.events.push("disarm".to_string());
            Ok(())
        }
    }

    fn active_lease(generation: &str, created: u64, expires: u64) -> LocalAccessLease {
        LocalAccessLease {
            version: 1,
            generation: generation.to_string(),
            created_at_epoch_seconds: created,
            expires_at_epoch_seconds: expires,
            active: true,
            revocation_pending: false,
        }
    }

    #[test]
    fn local_access_ttl_reuses_provider_neutral_duration_grammar() {
        assert_eq!(parse_local_access_ttl("30m").unwrap(), Duration::from_secs(30 * 60));
        assert_eq!(parse_local_access_ttl("2h").unwrap(), Duration::from_secs(2 * 60 * 60));
        assert_eq!(parse_local_access_ttl("2d").unwrap(), Duration::from_secs(2 * 24 * 60 * 60));
        assert_eq!(parse_local_access_ttl("45").unwrap(), Duration::from_secs(45));
        assert!(parse_local_access_ttl("").is_err());
        assert!(parse_local_access_ttl("0m").is_err());
        assert!(parse_local_access_ttl("30w").is_err());
    }

    #[test]
    fn local_tunnel_assets_pin_reviewed_release_and_keep_runtime_key_out_of_argv() {
        validate_local_tunnel_id(TEST_TUNNEL_ID).expect("valid tunnel id");
        for bad in [
            "",
            "0123456789abcdef0123456789abcdef",
            "tunnel_ABCDEF0123456789abcdef01234567",
            "tunnel_0123456789abcdef",
        ] {
            assert!(validate_local_tunnel_id(bad).is_err(), "accepted invalid tunnel ID {bad}");
        }

        let fragment = build_local_tunnel_install_fragment(TEST_TUNNEL_ID).expect("tunnel fragment");
        assert!(fragment.contains(LOCAL_TUNNEL_VERSION));
        assert!(fragment.contains(LOCAL_TUNNEL_ARCHIVE_URL));
        assert!(fragment.contains(LOCAL_TUNNEL_ARCHIVE_SHA256));
        assert!(fragment.contains("/etc/zodex/tunnel/version"));
        assert!(fragment.contains("api_key: file:/etc/zodex/tunnel/runtime-key"));
        assert!(fragment.contains("listen_addr: 127.0.0.1:18080"));
        assert!(fragment.contains("http://127.0.0.1:8080/mcp?key="));
        assert!(fragment.contains("systemctl disable zodex-tunnel.service"));
        assert!(!fragment.contains("systemctl enable zodex-tunnel.service"));

        let unit = include_str!("../local_zodex_tunnel.service");
        assert!(unit.contains("User=zodex-tunnel"));
        assert!(unit.contains("Group=zodex-tunnel"));
        assert!(unit.contains("Requires=zodex-local-network.service zodexd.service"));
        assert!(unit.contains("NetworkNamespacePath=/run/netns/zodex-agent"));
        assert!(unit.contains("CapabilityBoundingSet=\n"));
        assert!(unit.contains("AmbientCapabilities=\n"));
        assert!(unit.contains("NoNewPrivileges=yes"));
        let exec_start = unit.lines().find(|line| line.starts_with("ExecStart=")).unwrap();
        assert_eq!(
            exec_start,
            "ExecStart=/usr/local/bin/tunnel-client run --config /etc/zodex/tunnel/config.yaml"
        );
        assert!(!exec_start.contains("runtime-key"));
    }

    #[test]
    fn local_guest_setup_provisions_tunnel_but_leaves_ingress_inactive() {
        let sources = phase_three_target().setup_sources.unwrap();
        let script = build_local_guest_setup_script(&sources).expect("setup script");
        let stop = script
            .find("systemctl stop zodex-tunnel.service zodexd.service zodex-prd.service")
            .unwrap();
        let network = script.find("systemctl restart zodex-local-network.service").unwrap();
        let services = script.find("systemctl enable --now zodex-prd.service zodexd.service").unwrap();
        assert!(stop < network && network < services);
        assert!(script.contains("systemctl disable zodex-tunnel.service"));
        assert!(!script.contains("systemctl enable --now zodex-tunnel.service"));
        assert!(script.contains("rm -f /tmp/zodex-local-reader.pem /tmp/zodex-local-publisher.pem /tmp/zodex-local-tunnel-runtime-key"));
        assert!(script.contains("! command -v unzip"));
    }

    #[test]
    fn local_start_orders_runtime_tunnel_lease_and_supervisor() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        let mut runtime = FakeLocalLifecycleRuntime {
            now: 1_000,
            ..Default::default()
        };
        let lease = start_local_access_with_runtime(
            &mut runtime,
            &phase_three_target(),
            &lease_path,
            Duration::from_secs(120),
        )
        .expect("start should succeed");

        assert_eq!(lease.created_at_epoch_seconds, 1_000);
        assert_eq!(lease.expires_at_epoch_seconds, 1_120);
        assert!(lease.active);
        assert!(!lease.revocation_pending);
        assert_eq!(runtime.events[0], "prepare");
        assert_eq!(runtime.events[1], "start_tunnel");
        assert!(runtime.events[2].starts_with("arm:"));
        assert_eq!(
            load_local_access_lease(&lease_path).unwrap().unwrap(),
            lease
        );
    }

    #[test]
    fn local_start_reconciles_expired_lease_before_new_runtime() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        save_local_access_lease(&lease_path, &active_lease("old", 100, 200)).unwrap();
        let mut runtime = FakeLocalLifecycleRuntime {
            now: 300,
            ..Default::default()
        };
        start_local_access_with_runtime(
            &mut runtime,
            &phase_three_target(),
            &lease_path,
            Duration::from_secs(60),
        )
        .expect("expired lease should reconcile");

        assert_eq!(
            &runtime.events[..4],
            ["stop_tunnel", "stop_machine", "disarm", "prepare"]
        );
        assert_eq!(runtime.events[4], "start_tunnel");
    }

    #[test]
    fn local_start_renewal_replaces_generation_without_stale_worker_authority() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        save_local_access_lease(&lease_path, &active_lease("old-generation", 100, 1_000)).unwrap();
        let mut runtime = FakeLocalLifecycleRuntime {
            now: 500,
            ..Default::default()
        };
        let renewed = start_local_access_with_runtime(
            &mut runtime,
            &phase_three_target(),
            &lease_path,
            Duration::from_secs(600),
        )
        .expect("renewal should succeed");
        assert_ne!(renewed.generation, "old-generation");
        assert_eq!(renewed.expires_at_epoch_seconds, 1_100);
        assert_eq!(runtime.events[0], "prepare");
        assert_eq!(runtime.events[1], "start_tunnel");
        assert!(!runtime.events.iter().any(|event| event == "stop_machine"));
        assert_eq!(
            local_lease_worker_decision(Some(&renewed), "old-generation", 1_001),
            LocalLeaseWorkerDecision::Exit
        );
    }

    #[test]
    fn local_start_refuses_unready_or_pre_tunnel_setup_before_runtime_side_effects() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        let mut provisioning = phase_three_target();
        provisioning.setup_state = LocalSetupState::Provisioning;
        let mut runtime = FakeLocalLifecycleRuntime::default();
        assert!(
            start_local_access_with_runtime(
                &mut runtime,
                &provisioning,
                &lease_path,
                Duration::from_secs(60),
            )
            .is_err()
        );
        assert!(runtime.events.is_empty());

        let mut legacy = phase_three_target();
        legacy.setup_sources.as_mut().unwrap().tunnel_id = None;
        assert!(
            start_local_access_with_runtime(
                &mut runtime,
                &legacy,
                &lease_path,
                Duration::from_secs(60),
            )
            .is_err()
        );
        assert!(runtime.events.is_empty());
    }

    #[test]
    fn local_start_failure_revokes_access_and_never_leaves_active_new_lease() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        let mut runtime = FakeLocalLifecycleRuntime {
            now: 500,
            fail_start_tunnel: true,
            ..Default::default()
        };
        let error = start_local_access_with_runtime(
            &mut runtime,
            &phase_three_target(),
            &lease_path,
            Duration::from_secs(60),
        )
        .expect_err("tunnel failure must fail start")
        .to_string();
        assert!(error.contains("failed to start Secure MCP Tunnel"));
        assert_eq!(
            runtime.events,
            ["prepare", "start_tunnel", "stop_tunnel", "stop_machine", "disarm"]
        );
        assert!(load_local_access_lease(&lease_path).unwrap().is_none());
    }

    #[test]
    fn supervisor_arm_failure_rolls_back_tunnel_before_machine() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        let mut runtime = FakeLocalLifecycleRuntime {
            now: 700,
            fail_arm: true,
            ..Default::default()
        };
        let error = start_local_access_with_runtime(
            &mut runtime,
            &phase_three_target(),
            &lease_path,
            Duration::from_secs(60),
        )
        .expect_err("supervisor failure must revoke access")
        .to_string();
        assert!(error.contains("durable Local TTL supervisor"));
        let stop_tunnel = runtime.events.iter().position(|event| event == "stop_tunnel").unwrap();
        let stop_machine = runtime.events.iter().position(|event| event == "stop_machine").unwrap();
        assert!(stop_tunnel < stop_machine);
        let persisted = load_local_access_lease(&lease_path).unwrap().unwrap();
        assert!(!persisted.active);
        assert!(!persisted.revocation_pending);
    }

    #[test]
    fn manual_stop_reports_partial_machine_failure_as_revoked_pending_reconciliation() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        save_local_access_lease(&lease_path, &active_lease("current", 100, 1_000)).unwrap();
        let mut runtime = FakeLocalLifecycleRuntime {
            now: 200,
            fail_stop_machine: true,
            ..Default::default()
        };
        let error = revoke_local_access_with_runtime(&mut runtime, &lease_path, None, true)
            .expect_err("machine stop failure must be visible")
            .to_string();
        assert!(error.contains("access is revoked"));
        assert_eq!(runtime.events[0], "stop_tunnel");
        assert_eq!(runtime.events[1], "stop_machine");
        let persisted = load_local_access_lease(&lease_path).unwrap().unwrap();
        assert!(!persisted.active);
        assert!(persisted.revocation_pending);
        assert_eq!(local_lease_view(Some(&persisted), 200), LocalLeaseView::RevocationPending);

        let mut retry = FakeLocalLifecycleRuntime::default();
        revoke_local_access_with_runtime(&mut retry, &lease_path, None, true)
            .expect("later reconciliation should complete");
        assert_eq!(retry.events, ["stop_tunnel", "stop_machine", "disarm"]);
        let reconciled = load_local_access_lease(&lease_path).unwrap().unwrap();
        assert!(!reconciled.active);
        assert!(!reconciled.revocation_pending);
    }

    #[test]
    fn stop_failures_preserve_possibly_active_truth_and_generation_guard() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("lease.json");
        save_local_access_lease(&lease_path, &active_lease("new", 100, 1_000)).unwrap();

        let mut stale = FakeLocalLifecycleRuntime::default();
        assert!(!revoke_local_access_with_runtime(&mut stale, &lease_path, Some("old"), false).unwrap());
        assert!(stale.events.is_empty(), "stale generation must have no side effects");

        let mut failing = FakeLocalLifecycleRuntime {
            fail_stop_tunnel: true,
            fail_stop_machine: true,
            ..Default::default()
        };
        assert!(
            revoke_local_access_with_runtime(&mut failing, &lease_path, Some("new"), false).is_err()
        );
        let persisted = load_local_access_lease(&lease_path).unwrap().unwrap();
        assert!(persisted.active);
        assert!(persisted.revocation_pending);
        assert_eq!(
            local_lease_view(Some(&persisted), 200),
            LocalLeaseView::PossiblyActiveRevocationPending
        );
    }

    #[test]
    fn lease_worker_is_generation_checked_and_absolute_time_based() {
        let lease = active_lease("current", 100, 200);
        assert_eq!(
            local_lease_worker_decision(Some(&lease), "stale", 150),
            LocalLeaseWorkerDecision::Exit
        );
        assert_eq!(
            local_lease_worker_decision(Some(&lease), "current", 150),
            LocalLeaseWorkerDecision::Wait(Duration::from_secs(5))
        );
        assert_eq!(
            local_lease_worker_decision(Some(&lease), "current", 205),
            LocalLeaseWorkerDecision::Revoke
        );
        let pending = LocalAccessLease {
            active: false,
            revocation_pending: true,
            ..lease
        };
        assert_eq!(
            local_lease_worker_decision(Some(&pending), "current", 150),
            LocalLeaseWorkerDecision::Revoke
        );
    }

    #[test]
    fn launchd_supervisor_contains_only_generation_and_operator_paths() {
        let plist = build_local_lease_launchd_plist(
            Path::new("/Applications/Zodex/zodex"),
            Path::new("/Users/operator"),
            "generation-safe",
        );
        assert!(plist.contains("com.ashray.zodex.local-lease"));
        assert!(plist.contains("<string>lease-worker</string>"));
        assert!(plist.contains("<string>generation-safe</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<key>SuccessfulExit</key>"));
        assert!(!plist.contains("runtime-key"));
        assert!(!plist.contains(TEST_TUNNEL_ID));
    }

    #[test]
    fn local_target_state_persists_tunnel_reference_not_secret_contents() {
        let temp = tempdir().unwrap();
        let target_path = temp.path().join("target.json");
        let target = phase_three_target();
        save_local_target_record(&target_path, &target).unwrap();
        let raw = fs::read_to_string(&target_path).unwrap();
        assert!(raw.contains(TEST_TUNNEL_ID));
        assert!(raw.contains("/tmp/tunnel-runtime-key"));
        assert!(!raw.contains("sk-test-runtime-secret"));
    }
