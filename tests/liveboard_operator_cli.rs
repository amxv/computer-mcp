use std::process::Command;

#[test]
fn local_watch_agent_filters_require_explicit_tui_mode() {
    for args in [
        ["local", "watch", "--agent", "k7m2"].as_slice(),
        ["local", "watch", "--all"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run zodex {args:?}: {error}"));
        assert!(
            !output.status.success(),
            "TUI-only watch filter unexpectedly parsed without --tui: zodex {args:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("--tui"),
            "expected clap to identify the required --tui mode for zodex {args:?}: {stderr}"
        );
    }
}

#[test]
fn local_watch_no_open_is_web_only() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "watch", "--tui", "--no-open"])
        .output()
        .expect("run zodex local watch --tui --no-open");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--no-open"));
    assert!(stderr.contains("--tui"));
}
