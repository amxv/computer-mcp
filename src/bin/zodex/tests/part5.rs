    use super::operator_guest::{
        OPERATOR_GUEST_MAX_TRANSFER_BYTES, OperatorGuestTarget, OperatorGuestTransport,
        resolve_github_mode_target_from_state, sprite_atomic_move_command,
        validate_operator_guest_transfer,
    };
    use super::{
        DEFAULT_CONFIG_PATH, GITHUB_MODE_REMOTE_TMP_PATH, GITHUB_MODE_STATE_PATH,
        GithubModeRecord, ResolvedSprite, disable_github_yolo_mode, enable_github_yolo_mode,
        local_machine_atomic_write_command,
    };
    use std::cell::RefCell;

    struct FakeOperatorGuest {
        identity: Vec<String>,
        state: RefCell<Option<String>>,
        staged: RefCell<Option<(String, Vec<u8>)>>,
        commands: RefCell<Vec<Vec<String>>>,
        writes: RefCell<Vec<(String, Vec<u8>)>>,
    }

    impl FakeOperatorGuest {
        fn new(identity: &[&str]) -> Self {
            Self {
                identity: identity.iter().map(|line| (*line).to_string()).collect(),
                state: RefCell::new(None),
                staged: RefCell::new(None),
                commands: RefCell::new(Vec::new()),
                writes: RefCell::new(Vec::new()),
            }
        }
    }

    impl OperatorGuestTransport for FakeOperatorGuest {
        fn exec_privileged(&self, command: &[String]) -> AnyResult<String> {
            self.commands.borrow_mut().push(command.to_vec());
            let script = command.join("\n");
            if script.contains("if sudo test -f /var/lib/zodex/mode/state.json") {
                return Ok(self.state.borrow().clone().unwrap_or_default());
            }
            if script.contains("sudo install -d -m 0750 -o zodex-publisher -g zodex")
                && script.contains(GITHUB_MODE_REMOTE_TMP_PATH)
            {
                let (path, bytes) = self
                    .staged
                    .borrow_mut()
                    .take()
                    .expect("mode install should have staged bytes");
                assert_eq!(path, GITHUB_MODE_REMOTE_TMP_PATH);
                *self.state.borrow_mut() = Some(String::from_utf8(bytes).expect("mode JSON utf8"));
                return Ok(String::new());
            }
            if script.contains(&format!("sudo rm -f {GITHUB_MODE_STATE_PATH}")) {
                *self.state.borrow_mut() = None;
                return Ok(String::new());
            }
            if script.contains("printf 'helper=%s\\n'") {
                return Ok(format!(
                    "helper={}\nuse_http_path=true\npush_rewrite=https://github.com/\n",
                    expected_zodex_agent_git_helper()
                ));
            }
            Ok(String::new())
        }

        fn write_file_atomic(&self, remote_path: &str, contents: &[u8]) -> AnyResult<()> {
            validate_operator_guest_transfer(remote_path, contents)?;
            self.writes
                .borrow_mut()
                .push((remote_path.to_string(), contents.to_vec()));
            *self.staged.borrow_mut() = Some((remote_path.to_string(), contents.to_vec()));
            Ok(())
        }

        fn identity_lines(&self) -> Vec<String> {
            self.identity.clone()
        }
    }

    fn one_sprite_registry() -> OperatorSpriteRegistry {
        OperatorSpriteRegistry {
            sprites: vec![OperatorSpriteRecord {
                name: "dev-sprite".to_string(),
                org: None,
                remote_config: DEFAULT_CONFIG_PATH.to_string(),
                last_setup_at: "2026-08-13T00:00:00Z".to_string(),
            }],
        }
    }

    #[test]
    fn github_mode_target_resolution_handles_explicit_inferred_missing_and_ambiguous_cases() {
        let local = phase_three_target();
        let registry = one_sprite_registry();

        assert_eq!(
            resolve_github_mode_target_from_state(true, None, None, None, &registry, Some(&local))
                .expect("explicit Local"),
            OperatorGuestTarget::Local
        );
        assert_eq!(
            resolve_github_mode_target_from_state(
                false,
                Some("explicit"),
                Some("team"),
                None,
                &registry,
                Some(&local),
            )
            .expect("explicit Sprite"),
            OperatorGuestTarget::Sprite(ResolvedSprite {
                name: "explicit".to_string(),
                org: Some("team".to_string()),
            })
        );
        assert_eq!(
            resolve_github_mode_target_from_state(
                false,
                None,
                None,
                None,
                &OperatorSpriteRegistry::default(),
                Some(&local),
            )
            .expect("Local-only inference"),
            OperatorGuestTarget::Local
        );
        assert_eq!(
            resolve_github_mode_target_from_state(false, None, None, None, &registry, None)
                .expect("Sprite-only inference"),
            OperatorGuestTarget::Sprite(ResolvedSprite {
                name: "dev-sprite".to_string(),
                org: None,
            })
        );
        assert_eq!(
            resolve_github_mode_target_from_state(
                false,
                None,
                None,
                Some("env-sprite"),
                &OperatorSpriteRegistry::default(),
                None,
            )
            .expect("environment Sprite inference"),
            OperatorGuestTarget::Sprite(ResolvedSprite {
                name: "env-sprite".to_string(),
                org: None,
            })
        );

        let ambiguous = resolve_github_mode_target_from_state(
            false,
            None,
            None,
            None,
            &registry,
            Some(&local),
        )
        .expect_err("Local plus Sprite must be ambiguous")
        .to_string();
        assert!(ambiguous.contains("--local"));
        assert!(ambiguous.contains("--sprite <name>"));

        let missing = resolve_github_mode_target_from_state(
            false,
            None,
            None,
            None,
            &OperatorSpriteRegistry::default(),
            None,
        )
        .expect_err("no targets should fail")
        .to_string();
        assert!(missing.contains("no eligible Zodex target"));
        assert!(missing.contains("zodex local setup"));
    }

    #[test]
    fn github_mode_target_resolution_keeps_org_sprite_specific_and_requires_ready_local() {
        let local = phase_three_target();
        let mut provisioning = local.clone();
        provisioning.setup_state = LocalSetupState::Provisioning;
        let registry = OperatorSpriteRegistry {
            sprites: vec![
                OperatorSpriteRecord {
                    name: "personal".to_string(),
                    org: None,
                    remote_config: DEFAULT_CONFIG_PATH.to_string(),
                    last_setup_at: "now".to_string(),
                },
                OperatorSpriteRecord {
                    name: "team-sprite".to_string(),
                    org: Some("team".to_string()),
                    remote_config: DEFAULT_CONFIG_PATH.to_string(),
                    last_setup_at: "now".to_string(),
                },
            ],
        };

        assert_eq!(
            resolve_github_mode_target_from_state(
                false,
                None,
                Some("team"),
                None,
                &registry,
                Some(&local),
            )
            .expect("org should select Sprite rather than count Local"),
            OperatorGuestTarget::Sprite(ResolvedSprite {
                name: "team-sprite".to_string(),
                org: Some("team".to_string()),
            })
        );
        assert!(
            resolve_github_mode_target_from_state(
                true,
                None,
                Some("team"),
                None,
                &registry,
                Some(&local),
            )
            .expect_err("org with Local should fail")
            .to_string()
            .contains("Sprite-specific")
        );
        assert!(
            resolve_github_mode_target_from_state(
                true,
                None,
                None,
                None,
                &OperatorSpriteRegistry::default(),
                Some(&provisioning),
            )
            .expect_err("provisioning Local target must not be eligible")
            .to_string()
            .contains("not ready")
        );
    }

    #[test]
    fn github_mode_target_resolution_preserves_multiple_sprite_fail_closed_behavior() {
        let registry = OperatorSpriteRegistry {
            sprites: vec![
                OperatorSpriteRecord {
                    name: "one".to_string(),
                    org: None,
                    remote_config: DEFAULT_CONFIG_PATH.to_string(),
                    last_setup_at: "now".to_string(),
                },
                OperatorSpriteRecord {
                    name: "two".to_string(),
                    org: None,
                    remote_config: DEFAULT_CONFIG_PATH.to_string(),
                    last_setup_at: "now".to_string(),
                },
            ],
        };
        let error = resolve_github_mode_target_from_state(false, None, None, None, &registry, None)
            .expect_err("multiple Sprites require explicit selection")
            .to_string();
        assert!(error.contains("multiple Sprites are configured"));
        assert!(error.contains("one"));
        assert!(error.contains("two"));
        assert!(error.contains("--sprite <name>"));
    }

    #[test]
    fn github_mode_shared_policy_is_transport_neutral_and_does_not_touch_local_lease() {
        let sprite = FakeOperatorGuest::new(&["sprite: fixture"]);
        let local = FakeOperatorGuest::new(&["local: zodex-local"]);
        let temp = tempdir().expect("tempdir");
        let lease_path = temp.path().join("lease.json");
        let lease = active_lease("independent", 100, u64::MAX - 1);
        save_local_access_lease(&lease_path, &lease).expect("save independent lease");
        let lease_before = fs::read(&lease_path).expect("read lease before");

        for target in [&sprite, &local] {
            enable_github_yolo_mode(
                target,
                &["amxv/zodex".to_string()],
                Some(Duration::from_secs(7_200)),
            )
            .expect("shared policy enable");
        }

        let sprite_commands = sprite.commands.borrow();
        let local_commands = local.commands.borrow();
        assert_eq!(*sprite_commands, *local_commands);
        assert!(sprite_commands.iter().flatten().all(|token| {
            !token.contains("zodex-local-network")
                && !token.contains("zodex-tunnel.service")
                && !token.contains("local start")
                && !token.contains("ip netns")
                && !token.contains("sudo -u zodex-agent")
        }));
        assert_eq!(sprite.writes.borrow().len(), 1);
        assert_eq!(local.writes.borrow().len(), 1);
        assert_eq!(sprite.writes.borrow()[0].0, GITHUB_MODE_REMOTE_TMP_PATH);
        assert_eq!(local.writes.borrow()[0].0, GITHUB_MODE_REMOTE_TMP_PATH);
        drop(local_commands);
        drop(sprite_commands);

        for target in [&sprite, &local] {
            let record: GithubModeRecord = serde_json::from_str(
                target
                    .state
                    .borrow()
                    .as_deref()
                    .expect("mode state should be installed"),
            )
            .expect("mode JSON");
            assert_eq!(record.repos, ["amxv/zodex"]);
            assert_eq!(record.repo_grants.len(), 1);
            assert!(record.expires_at_epoch_seconds.is_some());
        }
        assert_eq!(fs::read(&lease_path).expect("read lease after"), lease_before);

        disable_github_yolo_mode(&local).expect("shared policy disable");
        assert!(local.state.borrow().is_none());
        assert_eq!(fs::read(&lease_path).expect("read lease after disable"), lease_before);
    }

    #[test]
    fn operator_guest_transfers_are_bounded_atomic_and_do_not_enter_agent_namespace() {
        validate_operator_guest_transfer("/tmp/state.json", b"ok").expect("small transfer");
        assert!(validate_operator_guest_transfer("relative", b"ok").is_err());
        assert!(validate_operator_guest_transfer("/tmp/bad\npath", b"ok").is_err());
        assert!(
            validate_operator_guest_transfer(
                "/tmp/too-large",
                &vec![0_u8; OPERATOR_GUEST_MAX_TRANSFER_BYTES + 1],
            )
            .is_err()
        );

        let local = local_machine_atomic_write_command("/tmp/state.json");
        assert_eq!(local[0], "/bin/sh");
        assert!(local[2].contains("cat > \"$staging\""));
        assert!(local[2].contains("mv -f -- \"$staging\" \"$1\""));
        let local_joined = local.join(" ");
        for forbidden in ["ip netns", "systemctl", "runuser", "zodex-agent", "nft"] {
            assert!(!local_joined.contains(forbidden), "local transfer contains {forbidden}");
        }

        let sprite = sprite_atomic_move_command(
            "/tmp/state.json.zodex-upload-fixture",
            "/tmp/state.json",
        );
        assert_eq!(sprite[0], "/bin/sh");
        assert!(sprite[2].contains("mv -f"));
        assert!(!sprite.join(" ").contains("zodex-agent"));
    }

    #[test]
    fn operator_guest_identity_labels_keep_sprite_and_local_distinct() {
        let sprite = OperatorGuestTarget::Sprite(ResolvedSprite {
            name: "dev".to_string(),
            org: Some("team".to_string()),
        });
        assert_eq!(
            sprite.identity_lines(),
            ["sprite: dev".to_string(), "org: team".to_string()]
        );
        assert_eq!(
            OperatorGuestTarget::Local.identity_lines(),
            ["local: zodex-local".to_string()]
        );
    }
