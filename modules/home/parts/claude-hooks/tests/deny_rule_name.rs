//! deny の理由の先頭にルールの名前が `[<name>] ` として付くこと（振り返りが拒否をルールごとに数えるため）

use std::io::Write;
use std::process::{Command, Stdio};

fn reason(command: &str) -> String {
    let input = serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "/tmp" });
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["pre-tool-use", "--rule", "guard-git-add"])
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
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
#[ignore = "RED: deny の理由にルールの名前が付かない"]
fn deny_reason_starts_with_the_rule_name() {
    assert!(
        reason("git add -A").starts_with("[guard-git-add] "),
        "{}",
        reason("git add -A")
    );
}

#[test]
#[ignore = "RED: deny の理由にルールの名前が付かない"]
fn escape_without_a_reason_is_labeled_too() {
    let r = reason("ALLOW_GIT_ADD_ALL=x git add -A");
    assert!(
        r.contains("\n\n[guard-git-add] ") || r.starts_with("[guard-git-add] "),
        "{r}"
    );
}
