use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script_path(name: &str) -> PathBuf {
    repo_root().join("scripts").join(name)
}

#[test]
fn github_app_scripts_have_valid_bash_syntax() {
    let scripts = [
        "mint-gh-app-installation-token.sh",
        "protect-main-branch.sh",
    ];

    for script in scripts {
        let output = Command::new("bash")
            .arg("-n")
            .arg(script_path(script))
            .output()
            .expect("run bash -n");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("bash -n failed for {script}: {stderr}");
        }
    }
}
