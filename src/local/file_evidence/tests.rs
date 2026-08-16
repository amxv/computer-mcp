use std::fs;

use serde_json::json;
use tempfile::tempdir;

use crate::invocation::InvocationStart;

use super::{
    FileOperationHint, FileSnapshot, MAX_FILE_EVIDENCE_BYTES, complete_file_evidence,
    parse_file_capture_plans, prepare_file_evidence,
};

fn exec_start(workdir: &std::path::Path, command: &str) -> InvocationStart {
    InvocationStart::new("exec_command", json!({"cmd": command, "workdir": workdir}))
}

fn patch_start(workdir: &std::path::Path, patch: &str) -> InvocationStart {
    InvocationStart::new("apply_patch", json!({"patch": patch, "workdir": workdir}))
}

#[test]
fn apply_patch_capture_plan_covers_create_update_delete_move_and_spaces() {
    let dir = tempdir().unwrap();
    let patch = "*** Begin Patch\n*** Add File: new file.txt\n+new\n*** Update File: edit.txt\n@@\n-old\n+new\n*** Delete File: gone.txt\n*** Update File: from name.txt\n*** Move to: to name.txt\n@@\n-old\n+new\n*** End Patch\n";
    let plans = parse_file_capture_plans(&patch_start(dir.path(), patch)).unwrap();
    assert_eq!(plans.len(), 4);
    assert_eq!(plans[0].operation, FileOperationHint::Create);
    assert_eq!(plans[0].path_after, dir.path().join("new file.txt"));
    assert_eq!(plans[1].operation, FileOperationHint::Update);
    assert_eq!(plans[2].operation, FileOperationHint::Delete);
    assert_eq!(plans[3].operation, FileOperationHint::Move);
    assert_eq!(plans[3].path_before, dir.path().join("from name.txt"));
    assert_eq!(plans[3].path_after, dir.path().join("to name.txt"));
}

#[test]
fn move_capture_records_destination_state_before_execution() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("source.txt"), "source\n").unwrap();
    fs::write(dir.path().join("destination.txt"), "destination\n").unwrap();
    let patch = "*** Begin Patch\n*** Update File: source.txt\n*** Move to: destination.txt\n@@\n-source\n+updated\n*** End Patch\n";
    let pending = prepare_file_evidence(&patch_start(dir.path(), patch));
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].destination_before,
        Some(FileSnapshot::Text("destination\n".to_string()))
    );
}

#[test]
fn malformed_or_ambiguous_patch_fails_closed() {
    let dir = tempdir().unwrap();
    for patch in [
        "*** Begin Patch\n*** Move to: orphan.txt\n*** End Patch\n",
        "*** Begin Patch\n*** Mystery File: nope.txt\n*** End Patch\n",
        "*** Add File: missing-envelope.txt\n+x\n",
    ] {
        assert!(parse_file_capture_plans(&patch_start(dir.path(), patch)).is_none());
    }

    let end_of_file = "*** Begin Patch\n*** Update File: valid.txt\n@@\n-old\n+new\n*** End of File\n*** End Patch\n";
    assert!(parse_file_capture_plans(&patch_start(dir.path(), end_of_file)).is_some());
}

#[test]
fn shell_write_parser_accepts_only_narrow_heredoc_and_printf_shapes() {
    let dir = tempdir().unwrap();
    let cases = [
        (
            "cat > 'file one.txt' <<'EOF'\nhello\nEOF\n",
            FileOperationHint::Overwrite,
            "file one.txt",
        ),
        (
            "cat <<\"EOF\" >> \"file two.txt\"\nhello\nEOF",
            FileOperationHint::Append,
            "file two.txt",
        ),
        (
            "printf 'hello\\n' > 'file three.txt'",
            FileOperationHint::Overwrite,
            "file three.txt",
        ),
        (
            "printf -- \"hello\\n\" >> \"file four.txt\"",
            FileOperationHint::Append,
            "file four.txt",
        ),
    ];
    for (command, operation, path) in cases {
        let plans = parse_file_capture_plans(&exec_start(dir.path(), command)).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].operation, operation);
        assert_eq!(plans[0].path_after, dir.path().join(path));
    }

    for command in [
        "cat > file.txt <<EOF\nx\nEOF\necho trailing",
        "printf 'x' > file.txt && echo trailing",
        "printf 'x' > $TARGET",
        "cat \"$TARGET\" > file.txt <<EOF\nx\nEOF",
        "cat <<-EOF > file.txt\nx\nEOF",
    ] {
        assert!(parse_file_capture_plans(&exec_start(dir.path(), command)).is_none());
    }
}

#[test]
fn bounded_file_capture_preserves_missing_text_binary_and_oversize_states() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let start = exec_start(dir.path(), "printf 'hello' > target.txt");
    let pending = prepare_file_evidence(&start);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].before, FileSnapshot::Missing);

    fs::write(&target, "hello\n").unwrap();
    let completed = complete_file_evidence(&pending);
    assert_eq!(
        completed[0].after,
        FileSnapshot::Text("hello\n".to_string())
    );

    fs::write(&target, [0xff, 0xfe, 0xfd]).unwrap();
    let binary = prepare_file_evidence(&start);
    assert!(matches!(binary[0].before, FileSnapshot::Unavailable(_)));

    fs::write(&target, vec![b'x'; MAX_FILE_EVIDENCE_BYTES as usize + 1]).unwrap();
    let oversized = prepare_file_evidence(&start);
    assert!(matches!(oversized[0].before, FileSnapshot::Unavailable(_)));
}
