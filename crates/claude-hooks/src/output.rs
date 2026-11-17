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
/// どちらもなければ None（= 許可、出力なし）
pub fn render(findings: &[Finding], notices: &[String]) -> Option<Value> {
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
        assert!(render(&[], &[]).is_none());
    }

    #[test]
    fn notices_alone_become_system_message_without_decision() {
        let v = render(&[], &["n".into()]).unwrap();
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
        )
        .unwrap();
        let o = &v["hookSpecificOutput"];
        assert_eq!(o["permissionDecision"], "deny");
        assert_eq!(o["permissionDecisionReason"], "a\n\nb");
        assert_eq!(o["additionalContext"], "c");
    }
}
