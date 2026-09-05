//! ルールの判定結果と hook 出力 JSON

use serde_json::{Value, json};

pub enum Finding {
    /// ツール呼び出しを止める。文字列は Claude に返す理由
    Deny(String),
    /// 止めずに情報だけ渡す（warn-large-commit）
    Context(String),
}

/// 判定結果を 1 つの hookSpecificOutput にまとめる。何もなければ None（= 許可、出力なし）
pub fn render(findings: &[Finding]) -> Option<Value> {
    let denies: Vec<&str> = findings
        .iter()
        .filter_map(|f| match f {
            Finding::Deny(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    let contexts: Vec<&str> = findings
        .iter()
        .filter_map(|f| match f {
            Finding::Context(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    if denies.is_empty() && contexts.is_empty() {
        return None;
    }
    let mut out = json!({ "hookEventName": "PreToolUse" });
    if !denies.is_empty() {
        out["permissionDecision"] = json!("deny");
        out["permissionDecisionReason"] = json!(denies.join("\n\n"));
    }
    if !contexts.is_empty() {
        out["additionalContext"] = json!(contexts.join("\n\n"));
    }
    Some(json!({ "hookSpecificOutput": out }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_means_allow() {
        assert!(render(&[]).is_none());
    }

    #[test]
    fn deny_and_context_share_one_object() {
        let v = render(&[
            Finding::Context("c".into()),
            Finding::Deny("a".into()),
            Finding::Deny("b".into()),
        ])
        .unwrap();
        let o = &v["hookSpecificOutput"];
        assert_eq!(o["permissionDecision"], "deny");
        assert_eq!(o["permissionDecisionReason"], "a\n\nb");
        assert_eq!(o["additionalContext"], "c");
    }
}
