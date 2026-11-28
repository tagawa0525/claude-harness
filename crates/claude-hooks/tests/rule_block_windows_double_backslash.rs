//! block-windows-double-backslash が、Windows で、Bash ツールの書き換えで bash の結果が変わる 2 つ続きのバックスラッシュを
//! 止め、結果が変わらないものと、Windows でない機械のコマンドを通すこと。Windows の Bash ツールは、コマンドの文字列の
//! `"` の直前でないバックスラッシュの並びを、bash に渡す前に 2 つごとに 1 つにする（引用符つきのヒアドキュメント、
//! 引用符なしのヒアドキュメント、python -c、単引用符の引数のどれでも。#437）

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
    // 二重引用符の中でも、bash が特別に読む文字（$）の前と、3 つ以上の並びは、bash の結果が変わる
    r#"echo "a\\$HOME""#,
    r#"echo "a\\\x""#,
    // 二重引用符の中の $(…) は、引用符の外と同じに読む
    r#"x="$(grep 'a\\b' f)""#,
];

/// 書き換えられても bash の結果が変わらない書き方（Bash ツールは " の直前の並びを変えない。二重引用符の中の 2 つは、
/// bash が特別に読まない文字の前なら、1 つにされても bash が同じ 1 つにする）
#[cfg(windows)]
#[test]
fn a_double_backslash_that_does_not_change_what_bash_runs_passes() {
    for command in [
        r#"printf '%s\n' 'c\\"d'"#,
        r#"bash -c "echo \\\"hi\\\"""#,
        r#"grep -n "foo\\|bar" f"#,
        "cat <<EOF\nfoo\\\\|bar\nEOF",
        "# a\\\\b\necho a",
    ] {
        assert!(command.contains(r"\\"), "{command}");
        assert_eq!(deny_reason(command), None, "{command}");
    }
}

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
