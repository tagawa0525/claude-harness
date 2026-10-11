//! git を起動できないときに、claude-hooks が理由を標準エラーに出すこと

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// テストの間だけある一時のディレクトリ（テストが panic しても消す）
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "claude-hooks-git-spawn-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `git commit` の入力を claude-hooks に渡し、標準エラーを返す。rule が None なら全てのルールを走らせる。
/// path が None なら PATH を空にする（git を起動できない）
fn stderr_of(cwd: &Path, rule: Option<&str>, path: Option<&str>) -> String {
    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "git commit -m x"},
        "cwd": cwd,
    });
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
    cmd.arg("pre-tool-use");
    if let Some(rule) = rule {
        cmd.args(["--rule", rule]);
    }
    // 呼んだ側のリポジトリ（git の hook の中の GIT_DIR など）や、上のディレクトリのリポジトリを見せない
    for (key, _) in std::env::vars() {
        if key.starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    let mut child = cmd
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .env("PATH", path.unwrap_or(""))
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
    let dir = TempDir::new("missing-git");
    let stderr = stderr_of(&dir.0, Some("block-main-commit"), None);
    assert!(stderr.contains("git を起動できません"), "stderr: {stderr}");
}

#[test]
fn git_missing_from_path_is_reported_once_across_rules() {
    let dir = TempDir::new("once");
    let stderr = stderr_of(&dir.0, None, None);
    assert_eq!(
        stderr.matches("git を起動できません").count(),
        1,
        "stderr: {stderr}"
    );
}

#[test]
fn missing_directory_is_not_reported() {
    let dir = TempDir::new("gone");
    let gone = dir.0.join("not-yet");
    assert_eq!(stderr_of(&gone, Some("block-main-commit"), None), "");
}

#[test]
fn git_failing_outside_a_repository_is_not_reported() {
    let dir = TempDir::new("not-a-repo");
    let path = std::env::var("PATH").expect("git を探す PATH が要る");
    assert_eq!(stderr_of(&dir.0, None, Some(&path)), "");
}
