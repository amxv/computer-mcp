use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

pub fn validate_absolute_existing_workdir(workdir: &str) -> Result<PathBuf> {
    let path = Path::new(workdir);
    if !path.is_absolute() {
        return Err(anyhow!("workdir must be an absolute path: {workdir}"));
    }
    if !path.exists() {
        return Err(anyhow!("workdir does not exist: {workdir}"));
    }
    if !path.is_dir() {
        return Err(anyhow!("workdir is not a directory: {workdir}"));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::validate_absolute_existing_workdir;

    #[test]
    fn accepts_absolute_existing_directory() {
        let dir = tempdir().expect("tempdir");
        let validated = validate_absolute_existing_workdir(dir.path().to_str().expect("utf8 path"))
            .expect("absolute directory should validate");
        assert_eq!(validated, dir.path());
    }

    #[test]
    fn rejects_relative_missing_and_file_paths() {
        let relative = validate_absolute_existing_workdir("relative/path")
            .expect_err("relative workdir must fail");
        assert!(relative.to_string().contains("absolute path"));

        let missing = validate_absolute_existing_workdir("/definitely/not/a/zodex/workdir")
            .expect_err("missing workdir must fail");
        assert!(missing.to_string().contains("does not exist"));

        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("file.txt");
        fs::write(&file, "x").expect("seed file");
        let not_dir = validate_absolute_existing_workdir(file.to_str().expect("utf8 path"))
            .expect_err("file workdir must fail");
        assert!(not_dir.to_string().contains("not a directory"));
    }
}
