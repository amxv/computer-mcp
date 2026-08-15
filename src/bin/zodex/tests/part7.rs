    #[test]
    fn local_operator_embedded_assets_materialize_without_repository_runtime_paths() {
        let temp = tempdir().expect("temp build context");
        let containerfile = super::materialize_local_machine_build_context(temp.path())
            .expect("materialize embedded Containerfile");
        let materialized = fs::read_to_string(&containerfile).expect("read materialized Containerfile");
        assert_eq!(materialized, super::LOCAL_MACHINE_CONTAINERFILE);
        assert!(materialized.contains("FROM debian:bookworm-slim"));
        assert!(materialized.contains("systemd-sysv"));
        assert!(materialized.contains("CMD [\"/sbin/init\"]"));

        let network_script = super::local_network::build_local_network_reconcile_script();
        let network_unit = super::local_network::local_network_unit();
        assert!(network_script.contains("zodex_local_filter"));
        assert!(network_unit.contains("ExecStart=/usr/local/libexec/zodex-local-network reconcile"));

        let sources = &phase_five_intent().setup_sources;
        let guest_setup = build_local_guest_setup_script(sources).expect("build embedded guest setup");
        for embedded_marker in [
            "[Unit]\nDescription=Zodex Local agent daemon",
            "[Unit]\nDescription=Zodex Local publisher daemon",
            "[Unit]\nDescription=Zodex Local Secure MCP Tunnel",
            "zodex-local-network.service",
        ] {
            assert!(
                guest_setup.contains(embedded_marker),
                "guest setup omitted embedded asset marker {embedded_marker:?}"
            );
        }

        let repo_root = env!("CARGO_MANIFEST_DIR");
        assert!(!guest_setup.contains(repo_root));
        assert!(!network_script.contains(repo_root));
        assert!(!network_unit.contains(repo_root));
        assert!(!materialized.contains(repo_root));
    }

    #[test]
    fn local_public_help_describes_target_setup_and_destructive_reset_boundaries() {
        let mut root = Cli::command();
        let root_help = root.render_long_help().to_string();
        assert!(root_help.contains("persistent isolated Apple Silicon Linux target"));

        let mut command = Cli::command();
        let local = command
            .find_subcommand_mut("local")
            .expect("local command");
        let local_help = local.render_long_help().to_string();
        assert!(local_help.contains("Permanently erase Local machine storage"));

        let setup = local
            .find_subcommand_mut("setup")
            .expect("local setup command");
        let setup_help = setup.render_long_help().to_string();
        assert!(setup_help.contains("GitHub repository in `owner/repo` form"));
        assert!(setup_help.contains("Pre-created OpenAI Secure MCP Tunnel ID"));
        assert!(setup_help.contains("Override the Local machine memory"));
    }
