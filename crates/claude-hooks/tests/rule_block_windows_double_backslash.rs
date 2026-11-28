//! block-windows-double-backslash が、Windows で 2 つ続きのバックスラッシュを含む Bash のコマンドを止め、1 つだけの
//! バックスラッシュと、Windows でない機械のコマンドを通すこと。Windows の Bash ツールは、コマンドの文字列の `\\` を
//! bash に渡す前に `\` 1 つにする（引用符つきのヒアドキュメント、引用符なしのヒアドキュメント、python -c、単引用符の
//! 引数のどれでも。#437）

use std::io::Write;
use std::process::{Command, Stdio};

/// command を block-windows-double-backslash だけに掛け、止めた理由を返す（止めなければ None）
fn deny_reason(command: &str) -> Option<String> {
    let input = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": env!("CARGO_TARGET_TMPDIR"),
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["pre-tool-use", "--rule", "block-windows-double-backslash"])
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
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    if stdout.trim().is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    json["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .map(str::to_string)
}

/// この機械で `\\` が `\` 1 つになると確かめた書き方
const COLLAPSED: &[&str] = &[
    "python - <<'EOF'\nprint(r'C:\\\\Users')\nEOF",
    "python - <<EOF\nprint('C:\\\\Users')\nEOF",
    "python -c \"print('C:\\\\Users')\"",
    "echo 'a\\\\b'",
    "sed 's/\\\\/\\//g' f",
];

#[cfg(windows)]
#[test]
fn a_double_backslash_is_denied_in_every_form_the_bash_tool_collapses() {
    for command in COLLAPSED {
        assert!(command.contains(r"\\"), "{command}");
        let reason = deny_reason(command).unwrap_or_else(|| panic!("通した: {command}"));
        assert!(reason.contains(r"\\"), "{reason}");
        assert!(reason.contains("Write"), "{reason}");
        assert!(reason.contains("C:/Users"), "{reason}");
    }
}

#[cfg(windows)]
#[test]
fn a_single_backslash_reaches_bash_as_written_and_passes() {
    for command in [
        "python - <<'EOF'\nprint('a\\nb')\nEOF",
        "python -c \"print('a\\tb')\"",
        "grep '\\.' f",
        "cat <<'EOF'\nC:\\Users\\tagaw\nEOF",
        "echo a",
    ] {
        assert_eq!(deny_reason(command), None, "{command}");
    }
}

#[cfg(not(windows))]
#[test]
fn a_double_backslash_passes_on_other_platforms() {
    for command in COLLAPSED {
        assert_eq!(deny_reason(command), None, "{command}");
    }
}
