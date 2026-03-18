use anyhow::{Result, anyhow};

pub fn apply_patch(patch: &str) -> Result<String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    match codex_apply_patch::apply_patch(patch, &mut stdout, &mut stderr) {
        Ok(()) => Ok(String::from_utf8_lossy(&stdout).into_owned()),
        Err(err) => {
            let stderr_text = String::from_utf8_lossy(&stderr).trim().to_string();
            if stderr_text.is_empty() {
                Err(anyhow!(err.to_string()))
            } else {
                Err(anyhow!(stderr_text))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::apply_patch;

    #[test]
    fn add_file_patch_creates_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("add.txt");
        let patch = format!(
            "*** Begin Patch\n*** Add File: {}\n+alpha\n+beta\n*** End Patch\n",
            path.display()
        );

        let output = apply_patch(&patch).expect("apply add patch");
        assert!(output.contains("Success. Updated the following files:"));
        assert!(output.contains(&format!("A {}", path.display())));
        assert_eq!(
            fs::read_to_string(&path).expect("read added file"),
            "alpha\nbeta\n"
        );
    }

    #[test]
    fn update_file_patch_modifies_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("update.txt");
        fs::write(&path, "old\n").expect("seed file");

        let patch = format!(
            "*** Begin Patch\n*** Update File: {}\n@@\n-old\n+new\n*** End Patch\n",
            path.display()
        );

        let output = apply_patch(&patch).expect("apply update patch");
        assert!(output.contains(&format!("M {}", path.display())));
        assert_eq!(
            fs::read_to_string(&path).expect("read updated file"),
            "new\n"
        );
    }

    #[test]
    fn delete_file_patch_removes_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("delete.txt");
        fs::write(&path, "to-delete\n").expect("seed file");

        let patch = format!(
            "*** Begin Patch\n*** Delete File: {}\n*** End Patch\n",
            path.display()
        );

        let output = apply_patch(&patch).expect("apply delete patch");
        assert!(output.contains(&format!("D {}", path.display())));
        assert!(!path.exists());
    }

    #[test]
    fn move_patch_renames_and_updates_file() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("moved").join("dst.txt");
        fs::write(&src, "line\n").expect("seed file");

        let patch = format!(
            "*** Begin Patch\n*** Update File: {}\n*** Move to: {}\n@@\n-line\n+line2\n*** End Patch\n",
            src.display(),
            dst.display()
        );

        let output = apply_patch(&patch).expect("apply move patch");
        assert!(output.contains(&format!("M {}", dst.display())));
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(&dst).expect("read moved file"),
            "line2\n"
        );
    }

    #[test]
    fn invalid_patch_returns_error() {
        let patch = "*** Begin Patch\n*** Add File: bad.txt\nbad-line\n*** End Patch\n";

        let err = apply_patch(patch).expect_err("invalid patch should fail");
        let message = err.to_string();
        assert!(
            message.contains("Invalid patch"),
            "unexpected error: {message}"
        );
    }
}
