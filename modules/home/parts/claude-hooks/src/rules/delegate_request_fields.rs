//! delegate-request-fields: /delegate を呼ぶ Agent の依頼に、定型の 6 つの欄がそろっているかを確かめる

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct DelegateRequestFields;

impl Rule for DelegateRequestFields {
    fn name(&self) -> &'static str {
        "delegate-request-fields"
    }

    fn tool(&self) -> &'static str {
        "Agent"
    }

    fn check(&self, _input: &Input, _shell: &Shell) -> Vec<Finding> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPLETE: &str = "最初に /delegate を呼び、その手順に従って進める。

リポジトリ: /home/u/repo
ブランチ: feat/x
何をするか: x を直す
前提:
- y は z（docs/a.md）
完成の形: テストが通る
変える範囲: src/
";

    fn check(prompt: &str, cwd: &std::path::Path) -> Vec<Finding> {
        let raw = serde_json::json!({
            "tool_name": "Agent",
            "cwd": cwd,
            "tool_input": { "prompt": prompt, "description": "d" },
        })
        .to_string();
        let input = Input::parse(&raw).unwrap();
        DelegateRequestFields.check(&input, &Shell::parse(""))
    }

    fn deny_reason(findings: &[Finding]) -> &str {
        match findings {
            [Finding::Deny(reason)] => reason,
            _ => panic!("deny が 1 つではない"),
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "claude-hooks-delegate-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pointer(path: &std::path::Path) -> String {
        format!(
            "依頼文は {} にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼んで、その手順に従って進める。",
            path.display()
        )
    }

    #[test]
    fn request_with_all_fields_passes() {
        assert!(check(COMPLETE, &temp_dir("all")).is_empty());
    }

    #[test]
    fn premise_heading_may_carry_a_parenthesized_qualifier() {
        let prompt = COMPLETE.replace(
            "前提:\n",
            "前提（出典で確かめたもの）:\n- a（b）\n前提（確かめていない。確かめることが作業の一部）:\n",
        );
        assert!(check(&prompt, &temp_dir("qualifier")).is_empty());
    }

    #[test]
    fn missing_fields_are_denied_by_name() {
        let prompt = COMPLETE
            .replace("前提:\n- y は z（docs/a.md）\n", "")
            .replace("完成の形: テストが通る\n", "");
        let findings = check(&prompt, &temp_dir("missing"));
        let reason = deny_reason(&findings);
        assert!(reason.contains("欠けた欄: 前提、完成の形"), "{reason}");
    }

    #[test]
    fn heading_must_start_the_line_and_be_followed_by_a_colon() {
        // 箇条書きの中の語や、コロンの無い語は見出しとして数えない
        let prompt = COMPLETE.replace("前提:\n", "- 前提: なし\n前提は下のとおり\n");
        let findings = check(&prompt, &temp_dir("line-start"));
        assert!(deny_reason(&findings).contains("欠けた欄: 前提"));
    }

    #[test]
    fn prompt_without_delegate_is_not_checked() {
        for prompt in [
            "/code-review high /home/u/repo-feat-x",
            "Explore the hooks crate and report where tool_name is read",
            "依頼に /delegated のような語があっても、/delegate を呼ぶ依頼ではない",
        ] {
            assert!(check(prompt, &temp_dir("other")).is_empty(), "{prompt}");
        }
    }

    #[test]
    fn delegate_request_without_any_field_lists_all_six() {
        let findings = check("最初に /delegate を呼ぶ。x を直す。", &temp_dir("none"));
        let reason = deny_reason(&findings);
        assert!(
            reason
                .contains("欠けた欄: リポジトリ、ブランチ、何をするか、前提、完成の形、変える範囲"),
            "{reason}"
        );
    }

    #[test]
    fn request_file_named_by_the_prompt_is_checked() {
        let dir = temp_dir("file");
        let complete = dir.join("brief-complete.md");
        std::fs::write(&complete, COMPLETE).unwrap();
        assert!(check(&pointer(&complete), &dir).is_empty());

        let partial = dir.join("brief-partial.md");
        std::fs::write(&partial, COMPLETE.replace("変える範囲: src/\n", "")).unwrap();
        let findings = check(&pointer(&partial), &dir);
        let reason = deny_reason(&findings);
        assert!(reason.contains("欠けた欄: 変える範囲"), "{reason}");
        assert!(reason.contains(&partial.display().to_string()), "{reason}");
    }

    #[test]
    fn request_file_in_backquotes_is_read() {
        let dir = temp_dir("backquote");
        let path = dir.join("brief.md");
        std::fs::write(&path, COMPLETE).unwrap();
        let prompt = format!(
            "依頼文は `{}` にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼ぶ。",
            path.display()
        );
        assert!(check(&prompt, &dir).is_empty());
    }

    #[test]
    fn missing_request_file_is_denied_with_its_path() {
        let dir = temp_dir("absent");
        let path = dir.join("no-such-brief.md");
        let findings = check(&pointer(&path), &dir);
        let reason = deny_reason(&findings);
        assert!(reason.contains(&path.display().to_string()), "{reason}");
        assert!(reason.contains("読めません"), "{reason}");
    }
}
