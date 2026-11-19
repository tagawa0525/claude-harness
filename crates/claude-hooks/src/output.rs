//! ルールの判定結果と hook 出力 JSON

use serde_json::{Value, json};
use std::sync::Mutex;

/// 判定できなかった理由（利用者に見せる）。git の呼び出しのように、ルールの戻り値を通らない深い所から足すので、
/// プロセスに 1 つ置く
static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 判定できなかった理由を足す。出力の systemMessage に入り、利用者の画面に出る（モデルには渡らない。ADR-0033）。
/// 1 回の判定で git や gh を何度も呼ぶので、同じ理由は 1 回だけにする
pub fn notice(message: String) {
    let mut notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
    if !notices.contains(&message) {
        notices.push(message);
    }
}

/// 判定できない理由を確かめるテストが持つロック。理由はプロセスに 1 つの置き場に集まり、take_notices は全部を
/// 取り出すので、理由を出すテストは（ルールのモジュールをまたいで）これを持って順に実行する
#[cfg(test)]
pub static NOTICES_LOCK: Mutex<()> = Mutex::new(());

/// これまでに足した理由を取り出す
pub fn take_notices() -> Vec<String> {
    std::mem::take(&mut *NOTICES.lock().unwrap_or_else(|e| e.into_inner()))
}

pub enum Finding {
    /// ツール呼び出しを止める。文字列は Claude に返す理由
    Deny(String),
    /// 止めずに情報だけ渡す（warn-large-commit）
    Context(String),
    /// 止めずに、Bash の command の前に `prefix` を付けた入力で実行させ、付けたことを `context` で Claude に知らせる
    /// （require-pipefail。ADR-0102）。許可の判断は返さず、許可の規則は書き換えた後の入力に対して評価される
    Prepend { prefix: String, context: String },
}

/// 止める理由の先頭にルールの名前を `[<name>] ` として付ける。振り返り（session-events）が、本文の番号やパスの違いに
/// 依らず拒否をルールごとに数えるための安定した印
pub fn label_denies(name: &str, findings: Vec<Finding>) -> Vec<Finding> {
    findings
        .into_iter()
        .map(|f| match f {
            Finding::Deny(s) => Finding::Deny(format!("[{name}] {s}")),
            other => other,
        })
        .collect()
}

/// 判定結果を 1 つの hookSpecificOutput に、判定できなかった理由を最上位の systemMessage にまとめる。
/// `tool_input` は hook の入力の tool_input で、書き換え（Prepend）があれば、その command の前に前置きを評価の順に
/// 付け、ほかの欄はそのままにして updatedInput にする（updatedInput は入力の全体を置き換えるため）。止める判定が
/// あれば書き換えは返さない（止めた呼び出しは走らないので、付けたという知らせも誤りになる）。
/// どれもなければ None（= 許可、出力なし）
pub fn render(findings: &[Finding], notices: &[String], tool_input: &Value) -> Option<Value> {
    let denies: Vec<&str> = findings
        .iter()
        .filter_map(|f| match f {
            Finding::Deny(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    let prepends: Vec<(&str, &str)> = if denies.is_empty() {
        findings
            .iter()
            .filter_map(|f| match f {
                Finding::Prepend { prefix, context } => Some((prefix.as_str(), context.as_str())),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let contexts: Vec<&str> = findings
        .iter()
        .filter_map(|f| match f {
            Finding::Context(s) => Some(s.as_str()),
            _ => None,
        })
        .chain(prepends.iter().map(|(_, context)| *context))
        .collect();
    if denies.is_empty() && contexts.is_empty() && notices.is_empty() {
        return None;
    }
    let mut result = json!({});
    if !denies.is_empty() || !contexts.is_empty() {
        let mut out = json!({ "hookEventName": "PreToolUse" });
        if !denies.is_empty() {
            out["permissionDecision"] = json!("deny");
            out["permissionDecisionReason"] = json!(denies.join("\n\n"));
        }
        if !prepends.is_empty() {
            let command = tool_input["command"].as_str().unwrap_or_default();
            let prefix: String = prepends.iter().map(|(prefix, _)| *prefix).collect();
            let mut updated = tool_input.clone();
            updated["command"] = json!(format!("{prefix}{command}"));
            out["updatedInput"] = updated;
        }
        if !contexts.is_empty() {
            out["additionalContext"] = json!(contexts.join("\n\n"));
        }
        result["hookSpecificOutput"] = out;
    }
    if !notices.is_empty() {
        result["systemMessage"] = json!(notices.join("\n"));
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_means_allow() {
        assert!(render(&[], &[], &json!({})).is_none());
    }

    #[test]
    fn notices_alone_become_system_message_without_decision() {
        let v = render(&[], &["n".into()], &json!({})).unwrap();
        assert_eq!(v["systemMessage"], "n");
        assert!(v.get("hookSpecificOutput").is_none());
    }

    #[test]
    fn denies_get_the_rule_name_and_contexts_do_not() {
        let labeled = label_denies(
            "r",
            vec![Finding::Deny("a".into()), Finding::Context("c".into())],
        );
        assert!(matches!(&labeled[0], Finding::Deny(s) if s == "[r] a"));
        assert!(matches!(&labeled[1], Finding::Context(s) if s == "c"));
    }

    #[test]
    fn deny_and_context_share_one_object() {
        let v = render(
            &[
                Finding::Context("c".into()),
                Finding::Deny("a".into()),
                Finding::Deny("b".into()),
            ],
            &[],
            &json!({}),
        )
        .unwrap();
        let o = &v["hookSpecificOutput"];
        assert_eq!(o["permissionDecision"], "deny");
        assert_eq!(o["permissionDecisionReason"], "a\n\nb");
        assert_eq!(o["additionalContext"], "c");
    }

    fn prepend(prefix: &str, context: &str) -> Finding {
        Finding::Prepend {
            prefix: prefix.into(),
            context: context.into(),
        }
    }

    #[test]
    fn prepend_rewrites_the_command_and_keeps_other_fields_without_a_decision() {
        let input = json!({ "command": "cargo test | tail", "run_in_background": true });
        let v = render(&[prepend("set -o pipefail; ", "p")], &[], &input).unwrap();
        let o = &v["hookSpecificOutput"];
        assert!(o.get("permissionDecision").is_none());
        assert_eq!(
            o["updatedInput"],
            json!({ "command": "set -o pipefail; cargo test | tail", "run_in_background": true })
        );
        assert_eq!(o["additionalContext"], "p");
    }

    #[test]
    fn prepend_joins_its_context_with_other_contexts() {
        let input = json!({ "command": "c" });
        let v = render(
            &[Finding::Context("c1".into()), prepend("a; ", "p")],
            &[],
            &input,
        )
        .unwrap();
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], "c1\n\np");
    }

    #[test]
    fn deny_drops_the_prepend_and_its_context() {
        let input = json!({ "command": "c" });
        let v = render(
            &[
                prepend("a; ", "p"),
                Finding::Deny("d".into()),
                Finding::Context("c1".into()),
            ],
            &[],
            &input,
        )
        .unwrap();
        let o = &v["hookSpecificOutput"];
        assert_eq!(o["permissionDecision"], "deny");
        assert!(o.get("updatedInput").is_none());
        assert_eq!(o["additionalContext"], "c1");
    }
}
