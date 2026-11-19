//! delegate-report-fields: /delegate の担当の報告に、「外したもの、後回しにしたもの」の欄があるかを確かめる（ADR-0098）
//!
//! 報告の書式（~/.claude/skills/delegate/references/working.md の手順 9）は、「親が先に読む要点」の直後にこの欄を置く。
//! 外して通した対処（検査や対象を外す、skip、条件を狭める、issue に残した指摘、測らずに置いた値）が成功の報告に
//! 埋もれないよう、親が先に読む位置に置き、無ければ「なし」と書かせる。欄の有無は報告の文字列だけで決まるので、
//! 送る前にここで判定する。
//!
//! - 対象: SendMessage と SubagentHandback の `message` に、行頭の `## 親が先に読む要点`（後ろに `（…）` の限定が続いても
//!   よい）がある呼び出し。途中の報告、親の指示、マージの結果はこの見出しを持たないので見ない
//! - コードブロック（行頭の ```` ``` ````）の中の行は見出しとして数えない（報告を引用する文を止めない）
//! - 判定: その次の `##` の見出しが `## 外したもの、後回しにしたもの`（後ろに `（…）` が続いてもよい）で、その中身
//!   （次の `##` の見出しか終わりまで）が空白だけでないこと
//!
//! 中身が外したものを全部書いているかは判定しない。エスケープはない（欄を足す以外に直す方法が無いため）。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct DelegateReportFields;

impl Rule for DelegateReportFields {
    fn name(&self) -> &'static str {
        "delegate-report-fields"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["SendMessage", "SubagentHandback"]
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "PR の許可を求めます。

## 親が先に読む要点
- 完成の形を満たした

## 外したもの、後回しにしたもの
なし

## ブランチと worktree
- リポジトリ: /r
";

    fn check(tool: &str, message: &str) -> Vec<Finding> {
        let raw = serde_json::json!({
            "tool_name": tool,
            "cwd": "/",
            "tool_input": { "to": "main", "message": message },
        })
        .to_string();
        let input = Input::parse(&raw).unwrap();
        DelegateReportFields.check(&input, &Shell::parse(""))
    }

    fn deny_reason(findings: &[Finding]) -> &str {
        match findings {
            [Finding::Deny(reason)] => reason,
            _ => panic!("deny が 1 つではない"),
        }
    }

    #[test]
    fn report_with_the_field_right_after_the_summary_passes() {
        assert!(check("SendMessage", REPORT).is_empty());
        assert!(check("SubagentHandback", REPORT).is_empty());
    }

    #[test]
    fn report_without_the_field_is_denied_with_the_fix() {
        let report = REPORT.replace("## 外したもの、後回しにしたもの\nなし\n\n", "");
        for tool in ["SendMessage", "SubagentHandback"] {
            let findings = check(tool, &report);
            let reason = deny_reason(&findings);
            assert!(reason.contains("外したもの、後回しにしたもの"), "{reason}");
            assert!(reason.contains("修正方法"), "{reason}");
        }
    }

    #[test]
    fn field_placed_after_another_section_is_denied() {
        let report = "## 親が先に読む要点
- x

## ブランチと worktree
- y

## 外したもの、後回しにしたもの
なし
";
        let findings = check("SendMessage", report);
        assert!(deny_reason(&findings).contains("直後"));
    }

    #[test]
    fn empty_field_is_denied() {
        let report = REPORT.replace(
            "## 外したもの、後回しにしたもの\nなし\n",
            "## 外したもの、後回しにしたもの\n  \n",
        );
        let findings = check("SendMessage", &report);
        assert!(deny_reason(&findings).contains("なし"));
    }

    #[test]
    fn empty_field_at_the_end_of_the_message_is_denied() {
        let report = "## 親が先に読む要点\n- x\n\n## 外したもの、後回しにしたもの\n";
        assert_eq!(check("SubagentHandback", report).len(), 1);
    }

    #[test]
    fn headings_may_carry_a_parenthesized_qualifier() {
        let report = REPORT
            .replace(
                "## 親が先に読む要点",
                "## 親が先に読む要点（管理のセッション向けの要約）",
            )
            .replace(
                "## 外したもの、後回しにしたもの",
                "## 外したもの、後回しにしたもの（この PR）",
            );
        assert!(check("SendMessage", &report).is_empty());
    }

    #[test]
    fn message_without_the_summary_heading_is_not_checked() {
        for message in [
            "レビューの待ちが 55 分を超えた。終わり次第、残りの作業を続ける",
            "報告の催促ではない。報告の「親が先に読む要点」に前提の欄が無いと書いた理由を教えて",
            "## 結果\nマージした: abc1234\n",
            "### 親が先に読む要点\n- x\n",
            "前置き ## 親が先に読む要点\n",
        ] {
            assert!(check("SendMessage", message).is_empty(), "{message}");
        }
    }

    #[test]
    fn report_quoted_in_a_code_block_is_not_checked() {
        let message = "前の報告をそのまま貼る:\n\n```markdown\n## 親が先に読む要点\n- x\n\n## ブランチと worktree\n```\n";
        assert!(check("SendMessage", message).is_empty());
    }

    #[test]
    fn headings_inside_a_code_block_do_not_count_as_the_next_section() {
        let report = "## 親が先に読む要点\n- x\n\n```text\n## 外したもの、後回しにしたもの\nなし\n```\n\n## ブランチと worktree\n";
        assert_eq!(check("SendMessage", report).len(), 1);
    }

    #[test]
    fn field_whose_body_is_only_a_code_block_passes() {
        let report = "## 親が先に読む要点\n- x\n\n## 外したもの、後回しにしたもの\n```text\nscripts/a.py を検査から外した\n```\n";
        assert!(check("SendMessage", report).is_empty());
    }
}
