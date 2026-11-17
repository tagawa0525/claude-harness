//! hook の入力の tool_name で、Agent の依頼の欄の検査が Agent の呼び出しにだけ掛かること

use std::io::Write;
use std::process::{Command, Stdio};

/// 欄の無い /delegate の依頼
const PROMPT: &str = "最初に /delegate を呼び、その手順に従って進める。x を直す。";

/// tool_name と tool_input を claude-hooks に渡し、stdout の JSON（空なら Null）を返す
fn run(tool_name: &str, tool_input: serde_json::Value) -> serde_json::Value {
    let input = serde_json::json!({ "tool_name": tool_name, "tool_input": tool_input, "cwd": "/" });
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .arg("pre-tool-use")
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
        serde_json::Value::Null
    } else {
        serde_json::from_str(&stdout).unwrap()
    }
}

#[test]
fn agent_request_without_fields_is_denied() {
    let out = run(
        "Agent",
        serde_json::json!({ "prompt": PROMPT, "description": "d" }),
    );
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = out["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(reason.contains("欠けた欄: リポジトリ"), "{reason}");
}

#[test]
fn other_tools_with_the_same_text_are_not_checked() {
    assert!(run("Read", serde_json::json!({ "prompt": PROMPT })).is_null());
    assert!(
        run(
            "Bash",
            serde_json::json!({ "command": format!("echo '{PROMPT}'") })
        )
        .is_null()
    );
}
