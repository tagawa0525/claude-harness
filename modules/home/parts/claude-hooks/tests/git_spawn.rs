//! git を起動できないときに、claude-hooks が理由を標準エラーに出すこと

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `git commit` の入力を PATH の空な環境で block-main-commit に渡し、標準エラーを返す
fn stderr_without_git(cwd: &Path) -> String {
    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "git commit -m x"},
        "cwd": cwd,
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["pre-tool-use", "--rule", "block-main-commit"])
        .env("PATH", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn git_missing_from_path_is_reported() {
    let dir = std::env::temp_dir().join(format!("claude-hooks-git-spawn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stderr = stderr_without_git(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(stderr.contains("git を起動できません"), "stderr: {stderr}");
}

#[test]
fn missing_directory_is_not_reported() {
    let dir = std::env::temp_dir().join(format!(
        "claude-hooks-git-spawn-missing-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(stderr_without_git(&dir), "");
}
