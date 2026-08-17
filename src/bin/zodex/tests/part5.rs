    #[test]
    fn github_device_code_preflight_parses_success_and_actionable_errors() {
        let success = super::parse_github_device_code_response(
            200,
            r#"{"device_code":"device-secret","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
        )
        .expect("valid device-code response");
        assert_eq!(success.user_code, "ABCD-EFGH");

        let disabled = super::parse_github_device_code_response(
            200,
            r#"{"error":"device_flow_disabled","error_description":"Device Flow is disabled"}"#,
        )
        .expect_err("disabled Device Flow must fail");
        assert!(disabled.to_string().contains("Device Flow is disabled"));

        let wrong_id = super::parse_github_device_code_response(
            401,
            r#"{"error":"incorrect_client_credentials","error_description":"bad client"}"#,
        )
        .expect_err("wrong client ID must fail");
        assert!(wrong_id.to_string().contains("Client ID"));
        assert!(wrong_id.to_string().contains("not the App ID"));
    }

    #[test]
    fn setup_requires_canonical_public_sprite_edge_and_writer_client_id_shape() {
        super::require_public_sprite_url_auth("public").expect("public auth");
        let err = super::require_public_sprite_url_auth("sprite").expect_err("private raw edge");
        assert!(err.to_string().contains("requires URL auth `public`"));

        super::validate_publisher_client_id("Iv1.writer-client_123")
            .expect("GitHub client ID shape");
        assert!(super::validate_publisher_client_id("").is_err());
        assert!(super::validate_publisher_client_id("client id with spaces").is_err());

        super::validate_default_base_branch("feature/safe").expect("valid branch");
        assert!(super::validate_default_base_branch("../bad").is_err());
    }

    #[test]
    fn managed_toml_string_literals_escape_valid_git_punctuation() {
        let literal = super::toml_string_literal("foo\"bar");
        let parsed: toml::Value = toml::from_str(&format!("value = {literal}\n"))
            .expect("serialized TOML literal must parse");
        assert_eq!(parsed["value"].as_str(), Some("foo\"bar"));
        super::validate_default_base_branch("foo\"bar").expect("valid Git punctuation");
        let script = super::build_sprite_setup_script(&super::SpriteSetupScriptOptions {
            repo: "amxv/zodex",
            reader_app_id: 10,
            reader_installation_id: 11,
            publisher_app_id: 20,
            publisher_client_id: "Iv1.writer-client",
            publisher_installation_id: 21,
            default_base: "foo\"bar",
            remote_config: Path::new("/etc/zodex/config.toml"),
        });
        assert!(script.contains(&format!("default_base = {literal}")));
        assert!(!script.contains("default_base = \"foo\"bar\""));
    }

    #[test]
    fn setup_missing_sprite_error_includes_explicit_create_action_without_auto_create() {
        assert_eq!(
            super::sprite_create_action("dev", None),
            "sprite create 'dev' --skip-console"
        );
        assert_eq!(
            super::sprite_create_action("dev", Some("team")),
            "sprite create -o 'team' 'dev' --skip-console"
        );
        assert_eq!(
            super::sprite_create_action("dev;echo bad", Some("team name")),
            "sprite create -o 'team name' 'dev;echo bad' --skip-console"
        );
    }

    #[test]
    fn runtime_health_parser_requires_component_status_and_version() {
        let parsed = super::parse_sprite_runtime_health(
            r#"{"status":"ok","component":"zodexd","version":"0.3.1"}"#,
        )
        .expect("runtime health");
        assert_eq!(parsed.component, "zodexd");
        assert_eq!(parsed.version, "0.3.1");

        assert!(super::parse_sprite_runtime_health(
            r#"{"status":"ok","component":"zodexd"}"#
        )
        .is_err());
        assert!(super::parse_sprite_runtime_health(
            r#"{"status":"degraded","component":"zodexd","version":"0.3.1"}"#
        )
        .is_err());

        super::validate_live_sprite_runtime_version(&parsed, "0.3.1")
            .expect("matching running version");
        let mismatch = super::validate_live_sprite_runtime_version(&parsed, "0.3.2")
            .expect_err("stale running process must fail");
        assert!(mismatch.to_string().contains("without restarting"));
    }

    #[test]
    fn resumable_setup_reuses_only_exact_current_registered_worker() {
        let proxy = super::OperatorSpriteProxyRecord {
            cloudflare_account_id: "acct".to_string(),
            worker_name: "zodex-dev".to_string(),
            worker_url: "https://zodex-dev.example.workers.dev".to_string(),
            worker_version: "v1".to_string(),
            worker_build: "build-a".to_string(),
            deployed_at: "2026-08-17T03:00:00Z".to_string(),
        };
        let resolution = ProxyOriginResolution {
            origin: "https://dev.example.sprites.app".to_string(),
            sprite_url_auth: Some("public".to_string()),
            sprite: None,
        };
        let current = ProxyWorkerStatus {
            component: "zodex-cloudflare-worker".to_string(),
            build: "build-a".to_string(),
            sprite_origin: Some(resolution.origin.clone()),
        };
        assert!(super::registered_proxy_matches_live_status(
            &proxy,
            &current,
            &resolution,
            "build-a"
        ));

        let stale = ProxyWorkerStatus {
            build: "build-old".to_string(),
            ..current.clone()
        };
        assert!(!super::registered_proxy_matches_live_status(
            &proxy,
            &stale,
            &resolution,
            "build-a"
        ));

        let wrong_origin = ProxyWorkerStatus {
            sprite_origin: Some("https://other.example.sprites.app".to_string()),
            ..current
        };
        assert!(!super::registered_proxy_matches_live_status(
            &proxy,
            &wrong_origin,
            &resolution,
            "build-a"
        ));
    }

    fn exact_tools_list_fixture() -> serde_json::Value {
        serde_json::json!({
            "jsonrpc":"2.0",
            "id":2,
            "result":{"tools":[
                {
                    "name":"exec_command",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "cmd":{"type":"string"},
                            "yield_time_ms":{"type":["integer","null"]},
                            "workdir":{"type":"string"},
                            "timeout_ms":{"type":["integer","null"]}
                        },
                        "required":["cmd","workdir"]
                    }
                },
                {
                    "name":"write_stdin",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "session_handle":{"type":"string"},
                            "chars":{"type":["string","null"]},
                            "yield_time_ms":{"type":["integer","null"]},
                            "kill_process":{"type":["boolean","null"]}
                        },
                        "required":["session_handle"]
                    }
                },
                {
                    "name":"apply_patch",
                    "inputSchema":{
                        "type":"object",
                        "properties":{
                            "patch":{"type":"string"},
                            "workdir":{"type":"string"}
                        },
                        "required":["patch","workdir"]
                    }
                }
            ]}
        })
    }

    #[test]
    fn mcp_health_contract_requires_exact_three_tool_argument_surface() {
        let fixture = exact_tools_list_fixture();
        super::validate_mcp_tool_contract(&fixture).expect("canonical MCP contract");

        let mut extra_tool = fixture.clone();
        extra_tool["result"]["tools"]
            .as_array_mut()
            .expect("tools")
            .push(serde_json::json!({
                "name":"dangerous_extra",
                "inputSchema":{"properties":{},"required":[]}
            }));
        assert!(super::validate_mcp_tool_contract(&extra_tool).is_err());

        let mut default_workdir = fixture.clone();
        let exec = default_workdir["result"]["tools"]
            .as_array_mut()
            .expect("tools")
            .iter_mut()
            .find(|tool| tool["name"] == "exec_command")
            .expect("exec");
        exec["inputSchema"]["properties"]["workdir"]["default"] =
            serde_json::json!("/workspace");
        let err = super::validate_mcp_tool_contract(&default_workdir)
            .expect_err("backend workdir default must fail");
        assert!(err.to_string().contains("workdir"));
    }

    #[test]
    fn mcp_health_response_parser_accepts_json_and_streamable_http_sse() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let parsed = super::parse_worker_mcp_response(
            Some("application/json; charset=utf-8"),
            json,
            Some(&serde_json::json!(1)),
        )
        .expect("plain JSON MCP response");
        assert_eq!(parsed["id"], 1);

        let sse = concat!(
            "data: \n",
            "id: 0\n",
            "retry: 3000\n",
            "\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n",
            "\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n",
            "\n",
        );
        let parsed = super::parse_worker_mcp_response(
            Some("text/event-stream"),
            sse,
            Some(&serde_json::json!(1)),
        )
        .expect("SSE MCP response");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["result"]["ok"], true);
    }

    #[test]
    fn mcp_health_response_parser_rejects_sse_without_expected_response_id() {
        let sse = concat!(
            "data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n",
            "\n",
        );
        let err = super::parse_worker_mcp_response(
            Some("text/event-stream; charset=utf-8"),
            sse,
            Some(&serde_json::json!(1)),
        )
        .expect_err("wrong JSON-RPC response id must fail");
        assert!(err.to_string().contains("expected JSON-RPC response"));
    }

    #[test]
    fn setup_cli_requires_writer_client_id_and_defaults_raw_edge_to_public() {
        let missing = Cli::try_parse_from([
            "zodex",
            "sprite",
            "setup",
            "--sprite",
            "dev",
            "--repo",
            "amxv/zodex",
            "--reader-app-id",
            "1",
            "--reader-pem",
            "/tmp/reader.pem",
            "--publisher-app-id",
            "2",
            "--publisher-pem",
            "/tmp/writer.pem",
        ])
        .expect_err("writer client ID is setup-critical");
        assert!(missing.to_string().contains("--publisher-client-id"));

        let parsed = Cli::try_parse_from([
            "zodex",
            "sprite",
            "setup",
            "--sprite",
            "dev",
            "--repo",
            "amxv/zodex",
            "--reader-app-id",
            "1",
            "--reader-pem",
            "/tmp/reader.pem",
            "--publisher-app-id",
            "2",
            "--publisher-client-id",
            "Iv1.writer",
            "--publisher-pem",
            "/tmp/writer.pem",
        ])
        .expect("canonical setup syntax");
        assert!(matches!(
            parsed.command,
            Commands::Sprite {
                command: SpriteCommand::Setup {
                    ref publisher_client_id,
                    ref url_auth,
                    ..
                }
            } if publisher_client_id == "Iv1.writer" && url_auth == "public"
        ));
    }

    #[test]
    fn setup_script_persists_writer_client_id_in_managed_config_block() {
        let script = super::build_sprite_setup_script(&super::SpriteSetupScriptOptions {
            repo: "amxv/zodex",
            reader_app_id: 10,
            reader_installation_id: 11,
            publisher_app_id: 20,
            publisher_client_id: "Iv1.writer-client",
            publisher_installation_id: 21,
            default_base: "main",
            remote_config: Path::new("/etc/zodex/config.toml"),
        });
        assert!(script.contains("# BEGIN ZODEX_GH_APPS_MANAGED"));
        assert!(script.contains("publisher_client_id = \"Iv1.writer-client\""));
        assert!(script.contains("service_port = 8080"));
        assert!(!script.contains("ZODEX_HTTP_BIND_PORT"));
    }

    #[test]
    fn upgrade_script_preserves_runtime_state_and_does_not_reset_grants_or_registry() {
        let script = super::build_sprite_upgrade_script(
            "latest",
            "amxv/zodex",
            Path::new("/etc/zodex/config.toml"),
        );
        assert!(script.contains("ZODEX_CONFIG_PATH=\"$CFG\""));
        assert!(!script.contains("push-grant"));
        assert!(!script.contains("yolo"));
        assert!(!script.contains("sprites.json"));
        assert!(!script.contains("rm -f \"$CFG\""));
        assert!(!script.contains("rm -rf /var/lib/zodex"));
    }
