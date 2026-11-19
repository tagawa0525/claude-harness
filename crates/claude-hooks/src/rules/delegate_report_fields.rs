//! delegate-report-fields: /delegate の担当の報告に、「外したもの、後回しにしたもの」の欄があるかを確かめる（ADR-0098）
//!
//! 報告の書式（~/.claude/skills/delegate/references/working.md の手順 9）は、「親が先に読む要点」の直後にこの欄を置く。
//! 外して通した対処（検査や対象を外す、skip、条件を狭める、issue に残した指摘、測らずに置いた値）が成功の報告に
//! 埋もれないよう、親が先に読む位置に置き、無ければ「なし」と書かせる。欄の有無は報告の文字列だけで決まるので、
//! 送る前にここで判定する。
//!
//! - 対象: SendMessage と SubagentHandback の `message` に、行頭（3 つまでの空白を許す）の `## 親が先に読む要点`（後ろに
//!   括弧の限定を 1 つ続けてもよい）がある呼び出し。途中の報告、親の指示、マージの結果はこの見出しを持たないので見ない
//! - コードブロック（` か ~ のフェンスの間）の中の行は見出しとして数えない（報告を引用する文を止めない）。閉じない
//!   フェンスがあれば、フェンスを見ずに数える（後ろの全部が隠れて、欄の無い報告が通るのを避ける）
//! - 判定: その次の `##` の見出しが `## 外したもの、後回しにしたもの`（同じく限定を続けてよい）で、その中身
//!   （次の `##` の見出しか終わりまで）が空白だけでないこと
//!
//! 中身が外したものを全部書いているかは判定しない。エスケープはない（欄を足す以外に直す方法が無いため）。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::script::dictionary_preview::fence;
use crate::shell::Shell;

pub struct DelegateReportFields;

/// 報告の最初の節の名前
const SUMMARY: &str = "親が先に読む要点";
/// その直後に置く節の名前
const DEFERRED: &str = "外したもの、後回しにしたもの";

/// 見出しの名前の部分（`## ` の後）が、name か、name に括弧（`（…）` か `(…)`）の限定を 1 つ続けたものか。閉じた限定の
/// 後に文が続く行は、その節の見出しとしない
fn is_named(title: &str, name: &str) -> bool {
    let Some(rest) = title.strip_prefix(name).map(str::trim_start) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    [('（', '）'), ('(', ')')].iter().any(|&(open, close)| {
        rest.starts_with(open) && rest.find(close) == Some(rest.len() - close.len_utf8())
    })
}

/// 行が `## ` の見出しなら、その名前の部分。Markdown と同じく、行頭の 3 つまでの空白を許す
fn heading_title(line: &str) -> Option<&str> {
    let body = line.trim_start_matches(' ');
    (line.len() - body.len() <= 3)
        .then(|| body.strip_prefix("## "))
        .flatten()
        .map(str::trim_end)
}

/// `## ` の見出しの、行の位置と名前の部分。fences が真なら、コードブロック（` か ~ を 3 つ以上並べたフェンスの間）の
/// 中の行を数えない
fn section_titles<'a>(lines: &[&'a str], fences: bool) -> Option<Vec<(usize, &'a str)>> {
    let mut open: Option<(char, usize)> = None;
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if fences {
            // フェンスの見分け方は dictionary_preview の文書の読み方と同じ（閉じるのは同じ文字で、開いたもの以上の長さの、
            // 後ろに何も無い行。` のフェンスの後ろに ` があれば、行頭のコードスパン）
            match (open, fence(line.trim())) {
                (Some((c, n)), Some((d, m, rest))) if c == d && m >= n && rest.is_empty() => {
                    open = None;
                    continue;
                }
                (Some(_), _) => continue,
                (None, Some((c, n, rest))) if c != '`' || !rest.contains('`') => {
                    open = Some((c, n));
                    continue;
                }
                (None, _) => {}
            }
        }
        if let Some(title) = heading_title(line) {
            found.push((i, title));
        }
    }
    // 閉じないフェンスは、後ろの全部を中身にして報告を見えなくする。その形では止める側に倒し、フェンスを見ずに数える
    open.is_none().then_some(found)
}

impl Rule for DelegateReportFields {
    fn name(&self) -> &'static str {
        "delegate-report-fields"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["SendMessage", "SubagentHandback"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        let lines: Vec<&str> = input.message.lines().collect();
        let titles = section_titles(&lines, true)
            .or_else(|| section_titles(&lines, false))
            .unwrap_or_default();
        let Some(summary) = titles.iter().position(|(_, t)| is_named(t, SUMMARY)) else {
            return Vec::new();
        };
        let next = titles.get(summary + 1);
        let problem = match next {
            Some((at, title)) if is_named(title, DEFERRED) => {
                let end = titles.get(summary + 2).map_or(lines.len(), |(e, _)| *e);
                if lines[at + 1..end].iter().all(|l| l.trim().is_empty()) {
                    format!("「{DEFERRED}」の節の中身が空です")
                } else {
                    return Vec::new();
                }
            }
            _ if titles.iter().any(|(_, t)| is_named(t, DEFERRED)) => format!(
                "「{DEFERRED}」の節が「{SUMMARY}」の直後にありません（直後は「{}」）",
                next.map_or("なし", |(_, t)| t)
            ),
            _ => format!("「{DEFERRED}」の節がありません"),
        };
        vec![Finding::Deny(format!(
            "/delegate の担当の報告（`## {SUMMARY}` で始まる節を持つメッセージ）で、{problem}。\n\
             外して通した対処が成功の報告に埋もれないよう、報告は「{SUMMARY}」の直後に「{DEFERRED}」の節を置きます（~/.claude/skills/delegate/references/working.md の手順 9）。\n\
             判定: コードブロック（``` か ~~~ のフェンスの間）の外の、行頭の `## ` の見出しを節とし、`## {SUMMARY}` の次の節が `## {DEFERRED}`（後ろに括弧の限定を 1 つ続けてもよい）で、中身が空白だけでないことを見ます。\n\
             修正方法: `## {SUMMARY}` の節の直後に `## {DEFERRED}` の節を置き、検査や対象から外したもの、skip、条件を狭めて通したもの、issue にして残したレビューの指摘、測らずに置いた値や組を書いて（無ければ「なし」と書いて）、送り直してください。報告の書式を引用するだけのメッセージなら、引用をコードブロック（```）に入れてください。"
        ))]
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

    #[test]
    fn code_span_at_the_start_of_a_line_does_not_open_a_code_block() {
        let report = REPORT.replace("- 完成の形を満たした", "```cargo test``` を流した");
        assert!(check("SendMessage", &report).is_empty());
    }

    #[test]
    fn report_quoted_in_a_tilde_code_block_is_not_checked() {
        let message = "前の報告:\n\n~~~markdown\n## 親が先に読む要点\n- x\n~~~\n";
        assert!(check("SendMessage", message).is_empty());
    }

    #[test]
    fn unclosed_code_block_before_the_summary_does_not_hide_the_report() {
        let report = format!(
            "```bash で実行した\n\n{}",
            REPORT.replace("## 外したもの、後回しにしたもの\nなし\n\n", "")
        );
        assert_eq!(check("SendMessage", &report).len(), 1);
    }

    #[test]
    fn qualifier_in_half_width_parentheses_is_accepted() {
        let report = REPORT
            .replace("## 親が先に読む要点", "## 親が先に読む要点 (要約)")
            .replace(
                "## 外したもの、後回しにしたもの",
                "## 外したもの、後回しにしたもの (この PR)",
            );
        assert!(check("SendMessage", &report).is_empty());
        let unfielded = REPORT
            .replace("## 親が先に読む要点", "## 親が先に読む要点 (要約)")
            .replace("## 外したもの、後回しにしたもの\nなし\n\n", "");
        assert_eq!(check("SendMessage", &unfielded).len(), 1);
    }

    #[test]
    fn heading_with_text_after_a_closed_qualifier_is_not_the_field() {
        let report = REPORT.replace(
            "## 外したもの、後回しにしたもの",
            "## 外したもの、後回しにしたもの（なし）以下は別の話",
        );
        assert_eq!(check("SendMessage", &report).len(), 1);
    }

    #[test]
    fn heading_indented_by_up_to_three_spaces_counts() {
        let report = REPORT
            .replace("## 外したもの、後回しにしたもの\nなし\n\n", "")
            .replace("## 親が先に読む要点", "   ## 親が先に読む要点");
        assert_eq!(check("SendMessage", &report).len(), 1);
    }

    #[test]
    fn deny_reason_does_not_cite_an_adr_number() {
        let report = REPORT.replace("## 外したもの、後回しにしたもの\nなし\n\n", "");
        let findings = check("SendMessage", &report);
        assert!(!deny_reason(&findings).contains("ADR-"));
    }
}
