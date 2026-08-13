    #[test]
    fn build_reader_status_lines_include_reader_hints() {
        let config = Config::default();
        let joined = build_reader_status_lines(&config).join("\n");
        assert!(joined.contains("service: zodex-reader"));
        assert!(joined.contains("active: not-ready"));
        assert!(joined.contains("hint: set `reader_app_id` in config"));
        assert!(joined.contains("hint: set `reader_installation_id` in config"));
    }

    #[test]
    fn parse_git_credential_request_extracts_known_fields() {
        let request = parse_git_credential_request(
            "protocol=https\nhost=github.com\npath=amxv/zodex.git\nusername=x-access-token\n\n",
        );

        assert_eq!(request.protocol.as_deref(), Some("https"));
        assert_eq!(request.host.as_deref(), Some("github.com"));
        assert_eq!(request.path.as_deref(), Some("amxv/zodex.git"));
        assert_eq!(request.username.as_deref(), Some("x-access-token"));
    }

    #[test]
    fn git_credential_request_targets_github_for_https_host() {
        let request = parse_git_credential_request("protocol=https\nhost=github.com\n\n");
        assert!(git_credential_request_targets_github(&request));
    }

    #[test]
    fn git_credential_request_targets_github_for_https_url_fallback() {
        let request = parse_git_credential_request("url=https://github.com/amxv/zodex.git\n\n");
        assert!(git_credential_request_targets_github(&request));
    }

    #[test]
    fn git_credential_request_rejects_non_github_or_non_https() {
        let ssh_request = parse_git_credential_request("protocol=ssh\nhost=github.com\n\n");
        let other_host_request =
            parse_git_credential_request("protocol=https\nhost=example.com\n\n");

        assert!(!git_credential_request_targets_github(&ssh_request));
        assert!(!git_credential_request_targets_github(&other_host_request));
    }

    #[test]
    fn credential_url_helpers_extract_protocol_and_host() {
        assert_eq!(
            credential_url_protocol("https://github.com/amxv/zodex.git"),
            Some("https")
        );
        assert_eq!(
            credential_url_host("https://token@github.com/amxv/zodex.git"),
            Some("github.com")
        );
        assert!(credential_host_is_github("github.com:443"));
        assert!(credential_host_is_github("www.github.com"));
        assert!(!credential_host_is_github("gitlab.com"));
    }

    #[test]
    fn github_repo_normalization_handles_git_suffix_and_url_path() {
        assert_eq!(
            normalize_github_repo("/amxv/zodex.git"),
            Some("amxv/zodex".to_string())
        );
        assert_eq!(
            credential_url_path("https://github.com/amxv/zodex.git"),
            Some("amxv/zodex.git")
        );
        assert_eq!(
            git_credential_request_repo(&parse_git_credential_request(
                "url=https://github.com/amxv/zodex.git\n\n"
            )),
            Some("amxv/zodex".to_string())
        );
    }

    #[test]
    fn normalize_github_repos_dedupes_repo_allowlist() {
        assert_eq!(
            normalize_github_repos(&[
                "amxv/zodex".to_string(),
                "/amxv/zodex.git".to_string(),
                "amxv/webctx".to_string(),
            ])
            .expect("repos should normalize"),
            vec!["amxv/zodex".to_string(), "amxv/webctx".to_string()]
        );
        assert!(normalize_github_repos(&["not-a-repo".to_string()]).is_err());
    }

    #[test]
    fn yolo_mode_record_defaults_to_all_installed_with_two_hour_ttl() {
        let record = build_github_yolo_mode_record(&[], Some(Duration::from_secs(2 * 60 * 60)))
            .expect("record should build");

        assert_eq!(record.mode, "yolo");
        assert!(record.all_installed);
        assert!(record.repos.is_empty());
        assert!(record.repo_grants.is_empty());
        assert_eq!(record.token_source, "publisher-app-installation-token");
        assert!(record.expires_at.is_some());
        assert!(record.expires_at_epoch_seconds.is_some());
    }

    #[test]
    fn yolo_mode_record_can_disable_ttl_and_scope_to_repos() {
        let record = build_github_yolo_mode_record(
            &["amxv/zodex".to_string(), "amxv/webctx.git".to_string()],
            None,
        )
        .expect("record should build");

        assert!(!record.all_installed);
        assert_eq!(
            record.repos,
            vec!["amxv/zodex".to_string(), "amxv/webctx".to_string()]
        );
        assert_eq!(
            record
                .repo_grants
                .iter()
                .map(|grant| grant.repo.as_str())
                .collect::<Vec<_>>(),
            vec!["amxv/zodex", "amxv/webctx"]
        );
        assert!(record.expires_at.is_none());
        assert!(record.expires_at_epoch_seconds.is_none());
        assert!(
            record
                .repo_grants
                .iter()
                .all(|grant| grant.expires_at_epoch_seconds.is_none())
        );
    }

    #[test]
    fn yolo_mode_expiration_is_enforced_by_epoch_cutoff() {
        let mut record = build_github_yolo_mode_record(&[], None).expect("record should build");
        record.expires_at_epoch_seconds = Some(1_000);

        assert!(!github_mode_expired(&record, 999));
        assert!(github_mode_expired(&record, 1_000));
    }

    #[test]
    fn yolo_repo_grants_merge_without_replacing_individual_ttls() {
        let first = build_github_yolo_mode_record_at(
            &["amxv/gooselake".to_string()],
            Some(Duration::from_secs(7_200)),
            1_000,
        )
        .expect("first record should build");
        let second = build_github_yolo_mode_record_at(
            &["amxv/agentbox".to_string()],
            Some(Duration::from_secs(7_200)),
            4_600,
        )
        .expect("second record should build");

        let merged = merge_github_yolo_mode_records(Some(first), second, 4_600);

        assert!(!merged.all_installed);
        assert_eq!(
            merged.repos,
            vec!["amxv/agentbox".to_string(), "amxv/gooselake".to_string()]
        );
        assert_eq!(merged.repo_grants.len(), 2);
        assert_eq!(merged.repo_grants[0].repo, "amxv/agentbox");
        assert_eq!(merged.repo_grants[0].expires_at_epoch_seconds, Some(11_800));
        assert_eq!(merged.repo_grants[1].repo, "amxv/gooselake");
        assert_eq!(merged.repo_grants[1].expires_at_epoch_seconds, Some(8_200));
    }

    #[test]
    fn yolo_repo_merge_prunes_expired_grants_and_refreshes_matching_repo() {
        let first = build_github_yolo_mode_record_at(
            &["amxv/gooselake".to_string(), "amxv/agentbox".to_string()],
            Some(Duration::from_secs(100)),
            1_000,
        )
        .expect("first record should build");
        let refreshed = build_github_yolo_mode_record_at(
            &["amxv/agentbox".to_string()],
            Some(Duration::from_secs(7_200)),
            1_200,
        )
        .expect("refreshed record should build");

        let merged = merge_github_yolo_mode_records(Some(first), refreshed, 1_200);

        assert_eq!(merged.repos, vec!["amxv/agentbox".to_string()]);
        assert_eq!(merged.repo_grants.len(), 1);
        assert_eq!(merged.repo_grants[0].repo, "amxv/agentbox");
        assert_eq!(merged.repo_grants[0].expires_at_epoch_seconds, Some(8_400));
    }

    #[test]
    fn yolo_agent_git_repair_script_sets_direct_push_plumbing() {
        let script = github_yolo_agent_git_repair_script();

        assert!(script.contains(r#"helper_cmd="/usr/local/bin/zodex-agent --config /etc/zodex/config.toml git-credential-helper""#));
        assert!(script.contains(r#"sudo -u zodex-agent env HOME="/home/zodex-agent" git config --global --replace-all credential.https://github.com.helper "$helper_cmd""#));
        assert!(script.contains(r#"sudo -u zodex-agent env HOME="/home/zodex-agent" git config --global credential.https://github.com.useHttpPath true"#));
        assert!(script.contains(r#"sudo -u zodex-agent env HOME="/home/zodex-agent" git config --global --replace-all url."zodex::https://github.com/".pushInsteadOf "https://github.com/""#));
        assert!(!script.contains(".insteadOf https://github.com/"));
    }

    #[test]
    fn yolo_agent_git_inspect_script_reads_direct_push_plumbing() {
        let script = github_yolo_agent_git_inspect_script();

        assert!(
            script.contains(
                r#"git config --global --get credential.https://github.com.helper || true"#
            )
        );
        assert!(script.contains(
            r#"git config --global --get credential.https://github.com.useHttpPath || true"#
        ));
        assert!(script.contains(r#"git config --global --get-all url."zodex::https://github.com/".pushInsteadOf || true"#));
        assert!(script.contains(r#"printf 'helper=%s\n' "$helper""#));
        assert!(script.contains(r#"printf 'use_http_path=%s\n' "$use_http_path""#));
        assert!(script.contains(r#"printf 'push_rewrite=%s\n' "$push_rewrite""#));
    }

    #[test]
    fn parse_yolo_agent_git_status_accepts_repaired_config() {
        let raw = format!(
            "helper={}\nuse_http_path=TRUE\npush_rewrite=https://example.com/\nhttps://github.com/\n",
            expected_zodex_agent_git_helper()
        );

        let status = parse_github_yolo_agent_git_status(&raw);

        assert_eq!(
            status,
            GithubYoloAgentGitStatus {
                helper: expected_zodex_agent_git_helper(),
                use_http_path: "TRUE".to_string(),
                push_rewrite: "https://example.com/\nhttps://github.com/".to_string(),
            }
        );
        assert!(status.helper_ok());
        assert!(status.use_http_path_ok());
        assert!(status.push_rewrite_ok());
        assert!(status.direct_push_ready());
        assert_eq!(
            build_github_yolo_agent_git_status_lines(&status),
            vec![
                "agent-git-helper: ok".to_string(),
                "agent-git-use-http-path: ok".to_string(),
                "agent-git-push-rewrite: ok".to_string(),
            ]
        );
    }

    #[test]
    fn parse_yolo_agent_git_status_reports_broken_config() {
        let status = parse_github_yolo_agent_git_status(
            "helper=/usr/bin/git-credential-store\nuse_http_path=false\npush_rewrite=https://example.com/\n",
        );

        assert_eq!(
            status,
            GithubYoloAgentGitStatus {
                helper: "/usr/bin/git-credential-store".to_string(),
                use_http_path: "false".to_string(),
                push_rewrite: "https://example.com/".to_string(),
            }
        );
        assert!(!status.helper_ok());
        assert!(!status.use_http_path_ok());
        assert!(!status.push_rewrite_ok());
        assert!(!status.direct_push_ready());
        assert_eq!(
            build_github_yolo_agent_git_status_lines(&status),
            vec![
                "agent-git-helper: missing-or-mismatched".to_string(),
                "agent-git-use-http-path: missing-or-mismatched".to_string(),
                "agent-git-push-rewrite: missing".to_string(),
            ]
        );
    }

    #[test]
    fn sprite_registry_resolves_explicit_env_single_and_ambiguous_cases() {
        let registry = OperatorSpriteRegistry {
            sprites: vec![OperatorSpriteRecord {
                name: "dev-sprite".to_string(),
                org: None,
                remote_config: "/etc/zodex/config.toml".to_string(),
                last_setup_at: "2026-06-30T00:00:00Z".to_string(),
            }],
        };

        let explicit = resolve_remote_sprite_from_registry(
            Some("explicit-sprite"),
            Some("team"),
            None,
            &registry,
        )
        .expect("explicit sprite should resolve");
        assert_eq!(explicit.name, "explicit-sprite");
        assert_eq!(explicit.org.as_deref(), Some("team"));

        let env_sprite =
            resolve_remote_sprite_from_registry(None, None, Some("env-sprite"), &registry)
                .expect("env sprite should resolve");
        assert_eq!(env_sprite.name, "env-sprite");

        let inferred = resolve_remote_sprite_from_registry(None, None, None, &registry)
            .expect("single registry sprite should resolve");
        assert_eq!(inferred.name, "dev-sprite");
        assert_eq!(inferred.org, None);

        let empty = OperatorSpriteRegistry::default();
        let empty_error = resolve_remote_sprite_from_registry(None, None, None, &empty)
            .expect_err("empty registry should require explicit sprite");
        let empty_message = empty_error.to_string();
        assert!(empty_message.contains("--sprite <name>"));
        assert!(empty_message.contains("ZODEX_SPRITE"));
        assert!(empty_message.contains("zodex sprite setup"));

        let ambiguous = OperatorSpriteRegistry {
            sprites: vec![
                OperatorSpriteRecord {
                    name: "one".to_string(),
                    org: None,
                    remote_config: "/etc/zodex/config.toml".to_string(),
                    last_setup_at: "2026-06-30T00:00:00Z".to_string(),
                },
                OperatorSpriteRecord {
                    name: "two".to_string(),
                    org: None,
                    remote_config: "/etc/zodex/config.toml".to_string(),
                    last_setup_at: "2026-06-30T00:00:00Z".to_string(),
                },
            ],
        };
        let ambiguous_error = resolve_remote_sprite_from_registry(None, None, None, &ambiguous)
            .expect_err("ambiguous registry should require explicit sprite");
        let ambiguous_message = ambiguous_error.to_string();
        assert!(ambiguous_message.contains("multiple Sprites are configured"));
        assert!(ambiguous_message.contains("one"));
        assert!(ambiguous_message.contains("two"));
        assert!(ambiguous_message.contains("--sprite <name>"));
        assert!(ambiguous_message.contains("ZODEX_SPRITE"));
    }

    #[test]
    fn sprite_registry_path_uses_zodex_config_dir() {
        assert_eq!(
            operator_sprites_registry_path_from_home(Path::new("/home/operator")),
            PathBuf::from("/home/operator/.config/zodex/sprites.json")
        );
    }

    #[test]
    fn sprite_registry_upsert_replaces_matching_sprite() {
        let mut registry = OperatorSpriteRegistry::default();
        upsert_operator_sprite_record(
            &mut registry,
            OperatorSpriteRecord {
                name: "dev".to_string(),
                org: None,
                remote_config: "/old".to_string(),
                last_setup_at: "old".to_string(),
            },
        );
        upsert_operator_sprite_record(
            &mut registry,
            OperatorSpriteRecord {
                name: "dev".to_string(),
                org: None,
                remote_config: "/new".to_string(),
                last_setup_at: "new".to_string(),
            },
        );

        assert_eq!(registry.sprites.len(), 1);
        assert_eq!(registry.sprites[0].remote_config, "/new");
    }

    #[test]
    fn matching_push_grant_uses_repo_path_and_ignores_ungranted_repo() {
        let grants_dir = tempdir().expect("tempdir");
        let granted_repo = "amxv/zodex";
        let grant_path = grants_dir.path().join("amxv__zodex.json");
        fs::write(
            &grant_path,
            r#"{"repo":"amxv/zodex","token":"push-token","expires_at":"2026-06-26T00:00:00Z"}"#,
        )
        .expect("write grant");

        let granted_request = parse_git_credential_request(
            "protocol=https\nhost=github.com\npath=amxv/zodex.git\n\n",
        );
        let ungranted_request = parse_git_credential_request(
            "protocol=https\nhost=github.com\npath=amxv/other.git\n\n",
        );

        let granted = load_matching_push_grant(&granted_request, grants_dir.path())
            .expect("granted lookup should succeed")
            .expect("grant should exist");
        let ungranted = load_matching_push_grant(&ungranted_request, grants_dir.path())
            .expect("ungranted lookup should succeed");

        assert_eq!(granted.repo, granted_repo);
        assert_eq!(granted.token, "push-token");
        assert!(ungranted.is_none());
    }

    #[test]
    fn parse_push_grant_ttl_accepts_common_units() {
        assert_eq!(
            parse_push_grant_ttl("30m").expect("30m should parse"),
            Duration::from_secs(30 * 60)
        );
        assert_eq!(
            parse_push_grant_ttl("2h").expect("2h should parse"),
            Duration::from_secs(2 * 60 * 60)
        );
        assert_eq!(
            parse_push_grant_ttl("2d").expect("2d should parse"),
            Duration::from_secs(2 * 24 * 60 * 60)
        );
        assert_eq!(
            parse_push_grant_ttl("45").expect("bare seconds should parse"),
            Duration::from_secs(45)
        );
    }

    #[test]
    fn parse_push_grant_ttl_rejects_empty_zero_and_unknown_units() {
        assert!(parse_push_grant_ttl("").is_err());
        assert!(parse_push_grant_ttl("0m").is_err());
        assert!(parse_push_grant_ttl("30w").is_err());
    }

    #[test]
    fn local_platform_support_is_runtime_classified() {
        assert_eq!(
            classify_local_platform("macos", "aarch64"),
            LocalPlatformSupport::Supported
        );
        assert!(matches!(
            classify_local_platform("linux", "aarch64"),
            LocalPlatformSupport::Unsupported(message) if message.contains("Apple Silicon macOS")
        ));
        assert!(matches!(
            classify_local_platform("macos", "x86_64"),
            LocalPlatformSupport::Unsupported(message) if message.contains("Apple Silicon")
        ));
    }

    #[test]
    fn local_provider_missing_is_distinct_from_unsupported() {
        let availability = probe_apple_provider_with(LocalPlatformSupport::Supported, |_program, _args| {
            Err(io::Error::new(io::ErrorKind::NotFound, "missing"))
        });
        assert_eq!(availability, LocalProviderAvailability::Missing);

        let unsupported = probe_apple_provider_with(
            LocalPlatformSupport::Unsupported("wrong platform".to_string()),
            |_program, _args| panic!("unsupported platforms must not invoke provider commands"),
        );
        assert_eq!(
            unsupported,
            LocalProviderAvailability::Unsupported("wrong platform".to_string())
        );
    }

    #[test]
    fn local_provider_version_and_home_mount_capability_are_classified_from_fixtures() {
        let version_fixture = r#"[{"version":"0.8.0","buildType":"release","commit":"abc123","appName":"container"}]"#;
        assert_eq!(
            parse_apple_system_version(version_fixture).expect("version fixture should parse"),
            "0.8.0"
        );

        let mut calls = 0;
        let ready = probe_apple_provider_with(LocalPlatformSupport::Supported, |_program, args| {
            calls += 1;
            if args == ["system", "version", "--format", "json"] {
                Ok(ProviderCommandOutput {
                    success: true,
                    stdout: version_fixture.to_string(),
                    stderr: String::new(),
                })
            } else if args == ["machine", "create", "--help"] {
                Ok(ProviderCommandOutput {
                    success: true,
                    stdout: "--home-mount <home-mount> User home mount (ro, rw, none)".to_string(),
                    stderr: String::new(),
                })
            } else {
                panic!("unexpected provider args: {args:?}");
            }
        });
        assert_eq!(calls, 2);
        assert_eq!(
            ready,
            LocalProviderAvailability::Ready {
                version: "0.8.0".to_string()
            }
        );

        let incompatible = probe_apple_provider_with(LocalPlatformSupport::Supported, |_program, args| {
            if args == ["system", "version", "--format", "json"] {
                Ok(ProviderCommandOutput {
                    success: true,
                    stdout: version_fixture.to_string(),
                    stderr: String::new(),
                })
            } else {
                Ok(ProviderCommandOutput {
                    success: true,
                    stdout: "machine create help without isolation option".to_string(),
                    stderr: String::new(),
                })
            }
        });
        assert!(matches!(
            incompatible,
            LocalProviderAvailability::Incompatible(message)
                if message.contains("--home-mount")
        ));
    }

    #[test]
    fn apple_machine_command_and_inspect_fixture_match_current_contract() {
        assert_eq!(
            apple_machine_inspect_args(LOCAL_MACHINE_NAME),
            vec!["machine", "inspect", "zodex-local"]
        );
        assert_eq!(
            apple_machine_create_help_args(),
            vec!["machine", "create", "--help"]
        );

        let machine = parse_apple_machine_inspect(
            r#"[{"id":"zodex-local","status":"running","cpus":12,"memory":34359738368,"homeMount":"none","diskSize":68719476736,"ipAddress":"192.0.2.2"}]"#,
        )
        .expect("current Apple machine inspect fixture should parse");
        assert_eq!(machine.id, LOCAL_MACHINE_NAME);
        assert!(machine_status_is_running(&machine.status));
        assert_eq!(machine.home_mount, "none");
        assert_eq!(
            classify_local_home_mount(&machine.home_mount),
            LocalHomeMountStatus::Isolated
        );
        assert_eq!(
            classify_local_home_mount("rw"),
            LocalHomeMountStatus::Unsafe("rw".to_string())
        );
        assert_eq!(format_bytes(machine.memory), "32 GiB");
    }

    #[test]
    fn local_machine_creation_is_no_home_mount_and_resource_scoped() {
        let args = local_machine_create_args(Some(12), Some("32G"));
        assert_eq!(
            args,
            vec![
                "machine",
                "create",
                "--no-boot",
                "--name",
                "zodex-local",
                "--home-mount",
                "none",
                "--cpus",
                "12",
                "--memory",
                "32G",
                "local/zodex-machine:1",
            ]
        );
        assert!(!args.iter().any(|arg| matches!(arg.as_str(), "--mount" | "--volume" | "-v")));

        assert_eq!(
            local_machine_set_args(Some(8), Some("16G")),
            vec![
                "machine",
                "set",
                "--name",
                "zodex-local",
                "home-mount=none",
                "cpus=8",
                "memory=16G",
            ]
        );
    }

    #[test]
    fn local_operator_exec_preserves_command_token_boundaries() {
        let command = vec![
            "sudo".to_string(),
            "printf".to_string(),
            "%s\\n".to_string(),
            "a b".to_string(),
            "$(not-a-shell)".to_string(),
        ];
        let args = local_machine_run_args(&command);
        assert_eq!(
            &args[..6],
            ["machine", "run", "--root", "--name", "zodex-local", "--"]
        );
        assert_eq!(&args[6..], command.as_slice());
    }

    #[test]
    fn local_setup_reconciles_owned_targets_and_rejects_unmanaged_machine() {
        let machine = parse_apple_machine_inspect(
            r#"[{"id":"zodex-local","status":"stopped","cpus":8,"memory":17179869184,"homeMount":"none"}]"#,
        )
        .expect("machine fixture");
        let state = LocalTargetRecord {
            version: 1,
            machine_id: LOCAL_MACHINE_NAME.to_string(),
            setup_state: LocalSetupState::Provisioning,
            image_reference: Some("local/zodex-machine:1".to_string()),
            requested_cpus: None,
            requested_memory: None,
            setup_sources: None,
        };

        assert_eq!(classify_local_setup_action(None, None), LocalSetupAction::Create);
        assert_eq!(
            classify_local_setup_action(None, Some(&machine)),
            LocalSetupAction::RejectUnmanaged
        );
        assert_eq!(
            classify_local_setup_action(Some(&state), Some(&machine)),
            LocalSetupAction::Reconcile
        );
        assert_eq!(
            classify_local_setup_action(Some(&state), None),
            LocalSetupAction::Create
        );
    }

    #[test]
    fn local_guest_setup_uses_runtime_installer_loopback_and_separate_identities() {
        let sources = LocalSetupSources {
            repo: "amxv/zodex".to_string(),
            reader_app_id: 11,
            reader_pem_path: "/Users/operator/reader.pem".to_string(),
            reader_installation_id: 12,
            publisher_app_id: 21,
            publisher_pem_path: "/Users/operator/publisher.pem".to_string(),
            publisher_installation_id: 22,
            default_base: "main".to_string(),
        };
        let script = build_local_guest_setup_script(&sources);

        assert!(script.contains("ZODEX_INSTALL_MODE=runtime"));
        assert!(script.contains("ZODEX_INSTALL_OPERATOR_CLI=0"));
        assert!(script.contains("bind_host = \\\"127.0.0.1\\\""));
        assert!(script.contains("User=zodex-agent"));
        assert!(script.contains("User=zodex-publisher"));
        assert!(script.contains("useradd --system --gid zodex-tunnel"));
        assert!(script.contains("/etc/zodex/tunnel"));
        assert!(!script.contains("/Users/operator/reader.pem"));
        assert!(!script.contains("/Users/operator/publisher.pem"));
        assert!(!script.contains("--volume"));
        assert!(!script.contains("--mount"));
    }

    #[test]
    fn local_setup_input_validation_is_conservative() {
        assert_eq!(validate_local_repo("amxv/zodex").unwrap(), "amxv/zodex");
        assert!(validate_local_repo("amxv/zodex;rm -rf /").is_err());
        assert!(validate_local_default_base("main").is_ok());
        assert!(validate_local_default_base("release/next").is_ok());
        assert!(validate_local_default_base("bad\"branch").is_err());
        assert!(validate_local_default_base("../escape").is_err());
    }

    #[test]
    fn local_setup_network_gate_fails_closed_until_host_policy_is_proven() {
        let error = local_host_network_isolation_gate()
            .expect_err("unproven host networking must block setup")
            .to_string();
        assert!(error.contains("public Internet"));
        assert!(error.contains("private-LAN"));
    }

    #[test]
    fn local_state_round_trips_with_atomic_private_files() {
        let temp = tempfile::tempdir().expect("temp dir");
        let target_path = local_target_state_path_from_home(temp.path());
        let lease_path = local_access_lease_path_from_home(temp.path());
        let target = LocalTargetRecord {
            version: 1,
            machine_id: LOCAL_MACHINE_NAME.to_string(),
            setup_state: LocalSetupState::Provisioning,
            image_reference: Some("local/zodex-machine:latest".to_string()),
            requested_cpus: Some(12),
            requested_memory: Some("32G".to_string()),
            setup_sources: Some(LocalSetupSources {
                repo: "amxv/zodex".to_string(),
                reader_app_id: 1,
                reader_pem_path: "/tmp/reader.pem".to_string(),
                reader_installation_id: 2,
                publisher_app_id: 3,
                publisher_pem_path: "/tmp/publisher.pem".to_string(),
                publisher_installation_id: 4,
                default_base: "main".to_string(),
            }),
        };
        save_local_target_record(&target_path, &target).expect("save target");
        assert_eq!(
            load_local_target_record(&target_path).expect("load target"),
            Some(target.clone())
        );

        let ready = LocalTargetRecord {
            setup_state: LocalSetupState::Ready,
            ..target
        };
        save_local_target_record(&target_path, &ready).expect("replace target atomically");
        assert_eq!(
            load_local_target_record(&target_path).expect("reload target"),
            Some(ready)
        );

        let lease = LocalAccessLease {
            version: 1,
            generation: "generation-1".to_string(),
            created_at_epoch_seconds: 100,
            expires_at_epoch_seconds: 200,
            active: true,
        };
        save_local_access_lease(&lease_path, &lease).expect("save lease");
        assert_eq!(
            load_local_access_lease(&lease_path).expect("load lease"),
            Some(lease)
        );

        #[cfg(unix)]
        {
            let target_mode = fs::metadata(&target_path).expect("target metadata").permissions().mode() & 0o777;
            let lease_mode = fs::metadata(&lease_path).expect("lease metadata").permissions().mode() & 0o777;
            assert_eq!(target_mode, 0o600);
            assert_eq!(lease_mode, 0o600);
        }
    }

    #[test]
    fn local_state_fails_closed_on_wrong_identity_or_invalid_lease() {
        let temp = tempfile::tempdir().expect("temp dir");
        let target_path = local_target_state_path_from_home(temp.path());
        let lease_path = local_access_lease_path_from_home(temp.path());
        fs::create_dir_all(target_path.parent().expect("target parent")).expect("state dir");

        fs::write(
            &target_path,
            r#"{"version":1,"machine_id":"other-machine","setup_state":"ready"}"#,
        )
        .expect("write wrong target identity");
        let target_error = load_local_target_record(&target_path)
            .expect_err("wrong machine identity must fail closed")
            .to_string();
        assert!(target_error.contains("expected `zodex-local`"));

        fs::write(
            &lease_path,
            r#"{"version":1,"generation":"generation-1","created_at_epoch_seconds":200,"expires_at_epoch_seconds":100,"active":true}"#,
        )
        .expect("write invalid lease");
        let lease_error = load_local_access_lease(&lease_path)
            .expect_err("backwards lease must fail closed")
            .to_string();
        assert!(lease_error.contains("expiration must be after creation"));

        fs::write(
            &target_path,
            r#"{"version":1,"machine_id":"zodex-local","setup_state":"ready"}"#,
        )
        .expect("write incomplete ready target");
        let ready_error = load_local_target_record(&target_path)
            .expect_err("ready target without setup sources must fail closed")
            .to_string();
        assert!(ready_error.contains("missing setup source references"));
    }

    #[test]
    fn push_grant_expired_only_when_epoch_cutoff_has_passed() {
        let active = PushGrantRecord {
            repo: "amxv/zodex".to_string(),
            token: "push-token".to_string(),
            expires_at: Some("2026-06-27T10:31:00Z".to_string()),
            expires_at_epoch_seconds: Some(1_000),
            token_source: Some("github-app-user-token".to_string()),
        };
        let no_ttl = PushGrantRecord {
            repo: "amxv/zodex".to_string(),
            token: "push-token".to_string(),
            expires_at: None,
            expires_at_epoch_seconds: None,
            token_source: Some("github-app-user-token".to_string()),
        };

        assert!(!push_grant_expired(&active, 999));
        assert!(push_grant_expired(&active, 1_000));
        assert!(!push_grant_expired(&no_ttl, 9_999));
    }

    #[test]
    fn load_push_grant_from_dir_ignores_expired_grants() {
        let grants_dir = tempdir().expect("tempdir");
        let path = grants_dir.path().join("amxv__zodex.json");
        fs::write(
            &path,
            r#"{"repo":"amxv/zodex","token":"push-token","expires_at":"1970-01-01T00:00:01Z","expires_at_epoch_seconds":1}"#,
        )
        .expect("write grant");

        let grant =
            load_push_grant_from_dir("amxv/zodex", grants_dir.path()).expect("lookup should work");

        assert!(grant.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn parse_push_grants_accepts_pretty_printed_grant_stream() {
        let first = PushGrantRecord {
            repo: "amxv/zodex".to_string(),
            token: "first-push-token".to_string(),
            expires_at: Some("2026-06-30T00:00:00Z".to_string()),
            expires_at_epoch_seconds: Some(1_782_777_600),
            token_source: Some("github-app-user-token".to_string()),
        };
        let second = PushGrantRecord {
            repo: "amxv/webctx".to_string(),
            token: "second-push-token".to_string(),
            expires_at: None,
            expires_at_epoch_seconds: None,
            token_source: Some("github-app-user-token".to_string()),
        };
        let raw = format!(
            "{}\n{}\n",
            serde_json::to_string_pretty(&first).expect("encode first grant"),
            serde_json::to_string_pretty(&second).expect("encode second grant")
        );

        let grants = parse_push_grants(&raw).expect("pretty grant stream should parse");

        assert_eq!(grants, vec![first, second]);
    }

    #[test]
    fn sprite_setup_and_upgrade_scripts_enable_github_use_http_path() {
        let setup_script = build_sprite_setup_script(
            "owner/repo",
            1,
            2,
            3,
            4,
            "main",
            Path::new("/etc/zodex/config.toml"),
        );
        let upgrade_script = build_sprite_upgrade_script(
            "latest",
            "owner/repo",
            Path::new("/etc/zodex/config.toml"),
        );

        assert!(setup_script.contains("credential.https://github.com.useHttpPath true"));
        assert!(upgrade_script.contains("credential.https://github.com.useHttpPath true"));
        assert!(setup_script.contains("url.\"zodex::https://github.com/\".pushInsteadOf"));
        assert!(upgrade_script.contains("url.\"zodex::https://github.com/\".pushInsteadOf"));
        assert!(!setup_script.contains(".insteadOf https://github.com/"));
        assert!(!upgrade_script.contains(".insteadOf https://github.com/"));
        let disabled_setting = ["credential.https://github.com.useHttpPath ", "false"].concat();
        assert!(!setup_script.contains(&disabled_setting));
        assert!(!upgrade_script.contains(&disabled_setting));
    }

    #[test]
    fn resolve_publisher_client_id_prefers_explicit_value_then_config() {
        let config = Config {
            publisher_client_id: Some("Iv1.from-config".to_string()),
            ..Config::default()
        };

        assert_eq!(
            resolve_publisher_client_id(&config, Some("Iv1.from-cli")),
            Some("Iv1.from-cli".to_string())
        );
        assert_eq!(
            resolve_publisher_client_id(&config, None),
            Some("Iv1.from-config".to_string())
        );
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
