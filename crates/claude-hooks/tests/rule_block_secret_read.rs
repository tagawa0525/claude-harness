//! block-secret-read が、Claude Code の設定（`CLAUDE_CONFIG_DIR` の settings.json）の permissions.deny の Read の規則を
//! 秘密の置き場の一覧として読み、Bash でそこを読むコマンドを止めること。設定が無ければ止めず、読めなければ止めずに
//! 理由を利用者に見せること。読み方の場合分けは src/rules/block_secret_read.rs の単体のテストが確かめる

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 設定の場所（CLAUDE_CONFIG_DIR）にする一時のディレクトリ
struct Config {
    dir: PathBuf,
}

impl Drop for Config {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Config {
    fn new(settings: Option<&str>) -> Config {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "block-secret-read-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(s) = settings {
            std::fs::write(dir.join("settings.json"), s).unwrap();
        }
        Config { dir }
    }

    /// command を block-secret-read だけに掛けた出力の JSON（出力が無ければ Null）
    fn run(&self, command: &str) -> serde_json::Value {
        let input = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": env!("CARGO_TARGET_TMPDIR"),
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["pre-tool-use", "--rule", "block-secret-read"])
            .env("CLAUDE_CONFIG_DIR", &self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{out:?}");
        if out.stdout.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

const SETTINGS: &str = r#"{"permissions": {"deny": ["Read(~/.ssh/**)", "Read(//**/.env)"]}}"#;

fn decision(out: &serde_json::Value) -> Option<&str> {
    out["hookSpecificOutput"]["permissionDecision"].as_str()
}

#[test]
fn reading_a_place_listed_in_the_settings_is_denied() {
    let config = Config::new(Some(SETTINGS));
    let out = config.run("echo ~/.ssh | xargs ls");
    assert_eq!(decision(&out), Some("deny"), "{out}");
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("Read(~/.ssh/**)"), "{reason}");
    assert_eq!(
        decision(&config.run("bash -c 'cat \"$(echo ~)/.ssh/id_ed25519\"'")),
        Some("deny")
    );
}

#[test]
fn places_not_listed_in_the_settings_are_allowed() {
    let config = Config::new(Some(SETTINGS));
    assert_eq!(
        config.run("cat ~/.aws/credentials"),
        serde_json::Value::Null
    );
}

#[test]
fn without_settings_nothing_is_denied() {
    let config = Config::new(None);
    assert_eq!(config.run("cat ~/.ssh/id_ed25519"), serde_json::Value::Null);
}

#[test]
fn unreadable_settings_are_shown_to_the_user_without_denying() {
    let config = Config::new(Some("{ not json"));
    let out = config.run("cat ~/.ssh/id_ed25519");
    assert_eq!(decision(&out), None, "{out}");
    let message = out["systemMessage"].as_str().unwrap_or_default();
    assert!(message.contains("settings.json"), "{out}");
}
