use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

const ADD_FILE_PREFIX: &str = "*** Add File: ";
const UPDATE_FILE_PREFIX: &str = "*** Update File: ";
const DELETE_FILE_PREFIX: &str = "*** Delete File: ";
const MOVE_TO_PREFIX: &str = "*** Move to: ";

pub fn apply_patch(patch: &str, workdir: &str) -> Result<String> {
    let rewritten_patch = rewrite_patch_paths(patch, workdir)?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    match codex_apply_patch::apply_patch(&rewritten_patch, &mut stdout, &mut stderr) {
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

fn rewrite_patch_paths(patch: &str, workdir: &str) -> Result<String> {
    let mut resolved = String::with_capacity(patch.len() + 128);
    let mut validated_workdir: Option<PathBuf> = None;

    for line in patch.lines() {
        if let Some(path) = line.strip_prefix(ADD_FILE_PREFIX) {
            let abs = resolve_patch_path(path, workdir, &mut validated_workdir)?;
            resolved.push_str(ADD_FILE_PREFIX);
            resolved.push_str(abs.to_string_lossy().as_ref());
            resolved.push('\n');
            continue;
        }
        if let Some(path) = line.strip_prefix(UPDATE_FILE_PREFIX) {
            let abs = resolve_patch_path(path, workdir, &mut validated_workdir)?;
            resolved.push_str(UPDATE_FILE_PREFIX);
            resolved.push_str(abs.to_string_lossy().as_ref());
            resolved.push('\n');
            continue;
        }
        if let Some(path) = line.strip_prefix(DELETE_FILE_PREFIX) {
            let abs = resolve_patch_path(path, workdir, &mut validated_workdir)?;
            resolved.push_str(DELETE_FILE_PREFIX);
            resolved.push_str(abs.to_string_lossy().as_ref());
            resolved.push('\n');
            continue;
        }
        if let Some(path) = line.strip_prefix(MOVE_TO_PREFIX) {
            let abs = resolve_patch_path(path, workdir, &mut validated_workdir)?;
            resolved.push_str(MOVE_TO_PREFIX);
            resolved.push_str(abs.to_string_lossy().as_ref());
            resolved.push('\n');
            continue;
        }

        resolved.push_str(line);
        resolved.push('\n');
    }

    Ok(resolved)
}

fn resolve_patch_path(
    path: &str,
    workdir: &str,
    validated_workdir: &mut Option<PathBuf>,
) -> Result<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    let workdir = validated_workdir
        .get_or_insert_with(|| PathBuf::from(workdir))
        .clone();
    if !workdir.exists() || !workdir.is_dir() {
        return Err(anyhow!(
            "apply_patch received relative patch path '{}' but workdir '{}' is invalid or not a directory",
            path.display(),
            workdir.display()
        ));
    }

    Ok(workdir.join(path))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

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

        let output =
            apply_patch(&patch, dir.path().to_string_lossy().as_ref()).expect("apply add patch");
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

        let output =
            apply_patch(&patch, dir.path().to_string_lossy().as_ref()).expect("apply update patch");
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

        let output =
            apply_patch(&patch, dir.path().to_string_lossy().as_ref()).expect("apply delete patch");
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

        let output =
            apply_patch(&patch, dir.path().to_string_lossy().as_ref()).expect("apply move patch");
        assert!(output.contains(&format!("M {}", dst.display())));
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(&dst).expect("read moved file"),
            "line2\n"
        );
    }

    #[test]
    fn relative_paths_resolve_from_workdir() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("sub");
        fs::create_dir_all(&nested).expect("create nested dir");
        let patch = "*** Begin Patch\n*** Add File: sub/relative.txt\n+hello\n*** End Patch\n";

        let output = apply_patch(patch, dir.path().to_string_lossy().as_ref())
            .expect("apply relative patch");
        let path = nested.join("relative.txt");
        assert!(output.contains(&format!("A {}", path.display())));
        assert_eq!(
            fs::read_to_string(path).expect("read added file"),
            "hello\n"
        );
    }

    #[test]
    fn absolute_paths_ignore_invalid_workdir() {
        let dir = tempdir().expect("tempdir");
        let absolute = dir.path().join("absolute.txt");
        let patch = format!(
            "*** Begin Patch\n*** Add File: {}\n+ok\n*** End Patch\n",
            absolute.display()
        );

        let output = apply_patch(&patch, "/definitely/not/a/real/workdir")
            .expect("absolute path patch should still apply");
        assert!(output.contains(&format!("A {}", absolute.display())));
        assert_eq!(
            fs::read_to_string(absolute).expect("read added file"),
            "ok\n"
        );
    }

    #[test]
    fn invalid_workdir_rejected_for_relative_paths() {
        let patch = "*** Begin Patch\n*** Add File: relative.txt\n+oops\n*** End Patch\n";
        let err = apply_patch(patch, "/definitely/not/a/real/workdir")
            .expect_err("relative patch should fail with invalid workdir");
        let message = err.to_string();
        assert!(message.contains("relative patch path"));
        assert!(message.contains("invalid or not a directory"));
    }

    #[test]
    fn invalid_patch_returns_error() {
        let patch = "*** Begin Patch\n*** Add File: bad.txt\nbad-line\n*** End Patch\n";

        let err = apply_patch(patch, Path::new(".").to_string_lossy().as_ref())
            .expect_err("invalid patch should fail");
        let message = err.to_string();
        assert!(
            message.contains("Invalid patch"),
            "unexpected error: {message}"
        );
    }
}
