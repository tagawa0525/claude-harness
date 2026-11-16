//! deny を状態ディレクトリの deny.jsonl に 1 件 1 行で追記すること（ADR-0090）

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `git add -A`（guard-git-add が止める）を、状態ディレクトリを `state` に向けて流す。判定の出力を返す
fn run(state: &Path, session: Option<&str>, command: &str, extra: &[(&str, &str)]) -> String {
    let mut input = serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "/tmp/work" });
    if let Some(s) = session {
        input["session_id"] = s.into();
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
    cmd.args(["pre-tool-use", "--rule", "guard-git-add"])
        .env("XDG_STATE_HOME", state)
        .env("LOCALAPPDATA", state)
        .env_remove("CLAUDE_HARNESS_DENY_LOG");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let mut child = cmd
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
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
}

fn lines(state: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(state.join("claude-harness/deny.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deny-log-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn deny_appends_one_json_line_with_the_fields() {
    let s = tmp("fields");
    run(&s, Some("sess-1"), "git add -A", &[]);
    run(&s, None, "git add -A", &[]);
    let l = lines(&s);
    assert_eq!(l.len(), 2);
    assert_eq!(l[0]["rule"], "guard-git-add");
    assert_eq!(l[0]["cwd"], "/tmp/work");
    assert_eq!(l[0]["session_id"], "sess-1");
    assert_eq!(l[0]["command"], "git add -A");
    assert!(!l[0]["reason"].as_str().unwrap().starts_with('['));
    assert!(l[1].get("session_id").is_none());
    let t = l[0]["time"].as_str().unwrap();
    let ok = t.len() == 20 && t.ends_with('Z') && t.as_bytes()[10] == b'T' && t.starts_with("20");
    assert!(ok, "{t}");
}

#[test]
fn command_and_reason_are_cut_to_200_chars() {
    let s = tmp("cut");
    let long = format!("git add -A # {}", "あ".repeat(300));
    run(&s, None, &long, &[]);
    let l = lines(&s);
    assert_eq!(l[0]["command"].as_str().unwrap().chars().count(), 200);
    assert!(l[0]["reason"].as_str().unwrap().chars().count() <= 200);
}

#[test]
fn allowed_command_is_not_logged() {
    let s = tmp("allow");
    run(&s, None, "git status", &[]);
    assert!(lines(&s).is_empty());
}

#[cfg(unix)]
#[test]
fn file_is_created_with_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let s = tmp("mode");
    run(&s, None, "git add -A", &[]);
    let m = std::fs::metadata(s.join("claude-harness/deny.jsonl"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(m & 0o777, 0o600);
}

#[test]
fn env_zero_disables_the_log() {
    let s = tmp("off");
    run(&s, None, "git add -A", &[("CLAUDE_HARNESS_DENY_LOG", "0")]);
    assert!(!s.join("claude-harness").exists());
}

#[test]
fn unwritable_state_dir_does_not_change_the_decision() {
    let s = tmp("unwritable");
    // 状態ディレクトリの場所がファイルなので、作れない
    let blocked = s.join("file");
    std::fs::write(&blocked, "").unwrap();
    let out = run(&blocked, None, "git add -A", &[]);
    assert!(out.contains("\"permissionDecision\":\"deny\""), "{out}");
}

#[cfg(unix)]
#[test]
fn existing_file_is_tightened_to_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let s = tmp("tighten");
    let f = s.join("claude-harness/deny.jsonl");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    run(&s, None, "git add -A", &[]);
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(lines(&s).len(), 1);
}

#[test]
fn concurrent_hooks_never_mix_lines() {
    let s = tmp("concurrent");
    let long = format!("git add -A # {}", "x".repeat(150));
    std::thread::scope(|sc| {
        for _ in 0..16 {
            sc.spawn(|| {
                for _ in 0..10 {
                    run(&s, None, &long, &[]);
                }
            });
        }
    });
    assert_eq!(lines(&s).len(), 160);
}

#[cfg(unix)]
#[test]
fn log_dir_is_created_and_tightened_to_mode_0700() {
    use std::os::unix::fs::PermissionsExt;
    let s = tmp("dirmode");
    run(&s, None, "git add -A", &[]);
    let d = s.join("claude-harness");
    assert_eq!(
        std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
        0o700
    );
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    run(&s, None, "git add -A", &[]);
    assert_eq!(
        std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
