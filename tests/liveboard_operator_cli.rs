use std::process::Command;

#[test]
fn local_watch_agent_is_valid_for_web_while_all_remains_tui_only() {
    let web = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "watch", "--agent", "k7m2"])
        .output()
        .expect("run zodex local watch --agent k7m2");
    let web_stderr = String::from_utf8_lossy(&web.stderr);
    assert!(
        !web_stderr.contains("required arguments were not provided")
            && !web_stderr.contains("--tui"),
        "web --agent should parse without requiring --tui: {web_stderr}"
    );

    let all = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "watch", "--all"])
        .output()
        .expect("run zodex local watch --all");
    assert!(!all.status.success());
    let all_stderr = String::from_utf8_lossy(&all.stderr);
    assert!(all_stderr.contains("--tui"));
}

#[test]
fn local_watch_rejects_invalid_web_agent_id_before_runtime_lookup() {
    let output = Command::new(env!("CARGO_BIN_EXE_zodex"))
        .args(["local", "watch", "--agent", "K7M2"])
        .output()
        .expect("run zodex local watch --agent K7M2");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Agent ID must be exactly four lowercase ASCII letters/digits"));
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
