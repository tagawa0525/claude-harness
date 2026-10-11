//! git を起動できないときに、claude-hooks が理由を systemMessage で利用者に見せること（ADR-0033）

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

/// claude-hooks の出力（stdout の JSON。空なら Null）と標準エラー
struct HookOutput {
    json: serde_json::Value,
    stderr: String,
}

impl HookOutput {
    /// 最上位の systemMessage（無ければ空）
    fn system_message(&self) -> &str {
        self.json["systemMessage"].as_str().unwrap_or("")
    }
}

/// command を claude-hooks に渡す。rule が None なら全てのルールを走らせる。
/// path が None なら PATH を空にする（git を起動できない）
fn run(cwd: &Path, command: &str, rule: Option<&str>, path: Option<&str>) -> HookOutput {
    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
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
        .stdout(Stdio::piped())
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    HookOutput {
        json: if stdout.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&stdout).unwrap()
        },
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn git_missing_from_path_is_shown_to_the_user() {
    let dir = TempDir::new("missing-git");
    let out = run(&dir.0, "git commit -m x", Some("block-main-commit"), None);
    assert!(
        out.system_message().contains("git を起動できません"),
        "json: {}",
        out.json
    );
    assert_eq!(out.stderr, "");
}

#[test]
fn git_missing_from_path_is_shown_once_across_rules() {
    let dir = TempDir::new("once");
    let out = run(&dir.0, "git commit -m x", None, None);
    assert_eq!(
        out.system_message().matches("git を起動できません").count(),
        1,
        "json: {}",
        out.json
    );
}

#[test]
fn deny_survives_alongside_the_message() {
    let dir = TempDir::new("with-deny");
    let out = run(&dir.0, "git commit -m x && sbatch job.sh", None, None);
    assert!(
        out.system_message().contains("git を起動できません"),
        "json: {}",
        out.json
    );
    assert_eq!(out.json["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[test]
fn unreadable_parent_directory_is_shown_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("locked");
    let inner = dir.0.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o000)).unwrap();
    let path = std::env::var("PATH").expect("git を探す PATH が要る");
    let out = run(
        &inner,
        "git commit -m x",
        Some("block-main-commit"),
        Some(&path),
    );
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        out.system_message().contains("git を起動できません"),
        "json: {}",
        out.json
    );
}

#[test]
fn missing_directory_is_not_reported() {
    let dir = TempDir::new("gone");
    let gone = dir.0.join("not-yet");
    let out = run(&gone, "git commit -m x", Some("block-main-commit"), None);
    assert_eq!(out.json, serde_json::Value::Null);
    assert_eq!(out.stderr, "");
}

#[test]
fn git_failing_outside_a_repository_is_not_reported() {
    let dir = TempDir::new("not-a-repo");
    let path = std::env::var("PATH").expect("git を探す PATH が要る");
    let out = run(&dir.0, "git commit -m x", None, Some(&path));
    assert_eq!(out.system_message(), "");
    assert_eq!(out.stderr, "");
}
