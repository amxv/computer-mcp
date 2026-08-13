    use super::local_recovery::{
        LocalResetRuntime, reset_local_with_runtime, resolve_local_reset_intent,
        validate_local_reset_intent,
    };
    use super::{
        LocalReadySetupIntent, load_local_ready_setup_intent,
        local_last_ready_setup_path_from_home, local_machine_configuration_needs_restart,
        local_machine_delete_args, local_ready_setup_intent_from_target, local_resource_drift_lines,
        local_runtime_ready_for_mcp, local_target_record_from_ready_intent, parse_local_memory_bytes,
        save_local_ready_setup_intent,
    };
    use super::local_lifecycle;

    fn phase_five_intent() -> LocalReadySetupIntent {
        local_ready_setup_intent_from_target(&phase_three_target()).expect("ready intent")
    }

    #[derive(Default)]
    struct FakeLocalResetRuntime {
        events: Vec<String>,
        fail_preflight: bool,
        fail_save_state: bool,
        fail_clear_lease: bool,
        fail_delete: bool,
        fail_reprovision: bool,
        saved: Option<LocalTargetRecord>,
        reprovisioned: Option<LocalTargetRecord>,
    }

    impl LocalResetRuntime for FakeLocalResetRuntime {
        fn preflight_recreation(&mut self, _intent: &LocalReadySetupIntent) -> anyhow::Result<()> {
            self.events.push("preflight".into());
            if self.fail_preflight {
                anyhow::bail!("injected preflight failure");
            }
            Ok(())
        }

        fn revoke_access(&mut self) -> anyhow::Result<()> {
            self.events.push("revoke".into());
            Ok(())
        }

        fn save_provisioning_state(&mut self, record: &LocalTargetRecord) -> anyhow::Result<()> {
            self.events.push("save_provisioning".into());
            if self.fail_save_state {
                anyhow::bail!("injected state persistence failure");
            }
            self.saved = Some(record.clone());
            Ok(())
        }

        fn delete_machine(&mut self) -> anyhow::Result<()> {
            self.events.push("delete".into());
            if self.fail_delete {
                anyhow::bail!("injected delete failure");
            }
            Ok(())
        }

        fn clear_lease(&mut self) -> anyhow::Result<()> {
            self.events.push("clear_lease".into());
            if self.fail_clear_lease {
                anyhow::bail!("injected lease cleanup failure");
            }
            Ok(())
        }

        fn reprovision(&mut self, record: LocalTargetRecord) -> anyhow::Result<()> {
            self.events.push("reprovision".into());
            self.reprovisioned = Some(record);
            if self.fail_reprovision {
                anyhow::bail!("injected reprovision failure");
            }
            Ok(())
        }
    }

    #[test]
    fn local_reset_preflights_before_every_destructive_step() {
        let intent = phase_five_intent();
        let mut runtime = FakeLocalResetRuntime::default();
        reset_local_with_runtime(&mut runtime, &intent).expect("reset transaction");
        assert_eq!(
            runtime.events,
            [
                "preflight",
                "revoke",
                "save_provisioning",
                "clear_lease",
                "delete",
                "reprovision",
            ]
        );
        let saved = runtime.saved.as_ref().expect("provisioning record");
        assert_eq!(saved.setup_state, LocalSetupState::Provisioning);
        assert_eq!(saved.requested_cpus, intent.requested_cpus);
        assert_eq!(saved.requested_memory, intent.requested_memory);
        assert_eq!(saved.network.as_ref(), Some(&intent.network));
        assert_eq!(saved.setup_sources.as_ref(), Some(&intent.setup_sources));
        assert_eq!(runtime.reprovisioned.as_ref(), Some(saved));

        let mut blocked = FakeLocalResetRuntime {
            fail_preflight: true,
            ..Default::default()
        };
        assert!(reset_local_with_runtime(&mut blocked, &intent).is_err());
        assert_eq!(blocked.events, ["preflight"]);
        assert!(blocked.saved.is_none());
    }

    #[test]
    fn local_reset_failure_boundaries_preserve_truthful_recovery_state() {
        let intent = phase_five_intent();
        let mut state_failure = FakeLocalResetRuntime {
            fail_save_state: true,
            ..Default::default()
        };
        assert!(reset_local_with_runtime(&mut state_failure, &intent).is_err());
        assert_eq!(
            state_failure.events,
            ["preflight", "revoke", "save_provisioning"]
        );

        let mut lease_failure = FakeLocalResetRuntime {
            fail_clear_lease: true,
            ..Default::default()
        };
        assert!(reset_local_with_runtime(&mut lease_failure, &intent).is_err());
        assert_eq!(
            lease_failure.events,
            ["preflight", "revoke", "save_provisioning", "clear_lease"]
        );
        assert_eq!(
            lease_failure.saved.unwrap().setup_state,
            LocalSetupState::Provisioning
        );

        let mut delete_failure = FakeLocalResetRuntime {
            fail_delete: true,
            ..Default::default()
        };
        let error = reset_local_with_runtime(&mut delete_failure, &intent)
            .expect_err("delete failure")
            .to_string();
        assert!(error.contains("delete failure"));
        assert_eq!(
            delete_failure.events,
            [
                "preflight",
                "revoke",
                "save_provisioning",
                "clear_lease",
                "delete",
            ]
        );
        assert_eq!(
            delete_failure.saved.unwrap().setup_state,
            LocalSetupState::Provisioning
        );

        let mut recreate_failure = FakeLocalResetRuntime {
            fail_reprovision: true,
            ..Default::default()
        };
        assert!(reset_local_with_runtime(&mut recreate_failure, &intent).is_err());
        assert_eq!(
            recreate_failure.events,
            [
                "preflight",
                "revoke",
                "save_provisioning",
                "clear_lease",
                "delete",
                "reprovision",
            ]
        );
        assert_eq!(
            recreate_failure.reprovisioned.unwrap().setup_state,
            LocalSetupState::Provisioning
        );
    }

    #[test]
    fn local_reset_missing_secret_source_fails_validation_before_runtime() {
        let temp = tempdir().expect("temp dir");
        let reader = temp.path().join("reader.pem");
        let publisher = temp.path().join("publisher.pem");
        let tunnel_key = temp.path().join("tunnel-key");
        fs::write(&reader, "reader").unwrap();
        fs::write(&publisher, "publisher").unwrap();
        fs::write(&tunnel_key, "runtime-key").unwrap();

        let mut intent = phase_five_intent();
        intent.setup_sources.reader_pem_path = reader.display().to_string();
        intent.setup_sources.publisher_pem_path = publisher.display().to_string();
        intent.setup_sources.tunnel_runtime_key_path = Some(tunnel_key.display().to_string());
        validate_local_reset_intent(&intent).expect("all saved sources readable");

        fs::remove_file(&tunnel_key).unwrap();
        let error = validate_local_reset_intent(&intent)
            .expect_err("missing secret source must block reset")
            .to_string();
        assert!(error.contains("saved tunnel runtime key"));

        let mut stale_image = phase_five_intent();
        stale_image.image_reference = "local/zodex-machine:old".into();
        let error = validate_local_reset_intent(&stale_image)
            .expect_err("stale embedded image identity must block reset")
            .to_string();
        assert!(error.contains("expects `local/zodex-machine:4`"));
    }

    #[test]
    fn interrupted_setup_can_recover_last_ready_intent_without_manual_state_editing() {
        let intent = phase_five_intent();
        let provisioning = local_target_record_from_ready_intent(
            &intent,
            LocalSetupState::Provisioning,
        );
        assert_eq!(
            resolve_local_reset_intent(Some(&provisioning), Some(intent.clone())).unwrap(),
            intent
        );
        let error = resolve_local_reset_intent(Some(&provisioning), None)
            .expect_err("never-ready provisioning state cannot invent reset intent")
            .to_string();
        assert!(error.contains("no last-ready recreation intent"));

        let ready = local_target_record_from_ready_intent(&intent, LocalSetupState::Ready);
        assert_eq!(
            resolve_local_reset_intent(Some(&ready), None).unwrap(),
            intent,
            "pre-Phase-5 ready state remains resettable"
        );
    }

    #[test]
    fn last_ready_setup_intent_is_private_atomic_and_independent_from_provisioning_state() {
        let temp = tempdir().unwrap();
        let path = local_last_ready_setup_path_from_home(temp.path());
        let intent = phase_five_intent();
        save_local_ready_setup_intent(&path, &intent).unwrap();
        assert_eq!(load_local_ready_setup_intent(&path).unwrap(), Some(intent));
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("runtime-secret-contents"));
        assert!(!raw.contains("github-token"));
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn reset_is_the_only_local_machine_command_that_deletes_persistent_storage() {
        assert_eq!(
            local_machine_delete_args(),
            ["machine", "delete", LOCAL_MACHINE_NAME]
        );
        let script = build_local_guest_setup_script(&phase_five_intent().setup_sources).unwrap();
        assert!(!script.contains("machine delete"));
        assert!(!script.contains("machine rm"));
        assert!(!script.contains("rm -rf /workspace"));
        assert!(!script.contains("rm -rf /home/zodex-agent"));
    }

    #[test]
    fn local_resource_intent_is_comparable_preserved_and_restart_aware() {
        assert_eq!(parse_local_memory_bytes("32G").unwrap(), 32 * 1024_u64.pow(3));
        assert_eq!(parse_local_memory_bytes("512MiB").unwrap(), 512 * 1024_u64.pow(2));
        assert!(parse_local_memory_bytes("0G").is_err());
        assert!(parse_local_memory_bytes("lots").is_err());

        let machine = parse_apple_machine_inspect(
            r#"[{"id":"zodex-local","status":"running","cpus":8,"memory":17179869184,"homeMount":"none"}]"#,
        )
        .unwrap();
        assert!(!local_machine_configuration_needs_restart(&machine, Some(8), Some("16G")).unwrap());
        assert!(local_machine_configuration_needs_restart(&machine, Some(12), Some("16G")).unwrap());
        assert!(local_machine_configuration_needs_restart(&machine, Some(8), Some("32G")).unwrap());

        let mut intent = phase_five_intent();
        intent.requested_cpus = Some(12);
        intent.requested_memory = Some("32G".into());
        let restored = local_target_record_from_ready_intent(&intent, LocalSetupState::Provisioning);
        assert_eq!(restored.requested_cpus, Some(12));
        assert_eq!(restored.requested_memory.as_deref(), Some("32G"));
        assert_eq!(
            local_resource_drift_lines(&restored, &machine),
            [
                "CPUs requested 12, observed 8",
                "memory requested 32G, observed 16 GiB",
            ]
        );
    }

    #[test]
    fn local_access_truth_fails_closed_on_external_runtime_or_isolation_drift() {
        assert!(local_runtime_ready_for_mcp(true, true, true, true));
        assert!(!local_runtime_ready_for_mcp(false, true, true, true));
        assert!(!local_runtime_ready_for_mcp(true, false, true, true));
        assert!(!local_runtime_ready_for_mcp(true, true, false, true));
        assert!(!local_runtime_ready_for_mcp(true, true, true, false));
    }

    #[test]
    fn network_repair_stops_model_services_before_the_one_reconciler_and_reverifies_membership() {
        let repair = local_lifecycle::local_network_repair_command();
        let shell = repair.last().expect("repair script");
        let stop = shell
            .find("systemctl stop zodex-tunnel.service zodexd.service zodex-prd.service")
            .unwrap();
        let reconcile = shell.find("systemctl restart zodex-local-network.service").unwrap();
        let start = shell.find("systemctl start zodex-prd.service zodexd.service").unwrap();
        assert!(stop < reconcile && reconcile < start);

        let verify = local_lifecycle::local_service_namespace_verify_command();
        let shell = verify.last().expect("service membership verify");
        for service in ["zodex-prd.service", "zodexd.service", "zodex-tunnel.service"] {
            assert!(shell.contains(service));
        }
        assert!(shell.contains("ip netns identify"));
        assert!(shell.contains("zodex-agent"));
    }

    #[test]
    fn stale_or_missing_lease_still_revokes_manually_started_runtime() {
        let dir = tempdir().unwrap();
        let lease_path = dir.path().join("missing-lease.json");
        let mut runtime = FakeLocalLifecycleRuntime::default();
        revoke_local_access_with_runtime(&mut runtime, &lease_path, None, true)
            .expect("missing lease does not skip fail-closed runtime stop");
        assert_eq!(runtime.events, ["stop_tunnel", "stop_machine", "disarm"]);
        assert!(!lease_path.exists());
    }
