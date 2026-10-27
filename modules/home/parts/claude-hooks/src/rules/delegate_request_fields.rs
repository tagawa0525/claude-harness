//! delegate-request-fields: /delegate を呼ぶ Agent の依頼に、定型の 6 つの欄がそろっているかを確かめる
//!
//! 依頼の定型（~/.claude/skills/delegate/SKILL.md の「頼む側（親のセッション）の書き方」）は、事実を「前提」の欄に
//! 1 行ずつ分けて書かせる。欄が抜けると、書く側は出典を開いたかを見返せず、受ける側は本文から事実を拾い出す。
//! 規則の文では漏れが止まらなかったので、欄の有無をここで機械で判定する。
//!
//! - 対象: prompt に `/delegate` の語がある Agent の呼び出し。`/code-review` や Explore などへの依頼は見ない
//! - 依頼の置き場: prompt に「依頼文は <絶対パス> にある」があれば、そのファイルを読んで判定する。無ければ prompt を判定する
//! - 見出し: 行頭が欄の名前で始まり、直後に `:` か `（…）:` が続く行（`前提:`、`前提（確かめていない）:`）
//!
//! 欄の中身（出典の有無、本文の事実）は判定しない。エスケープはない（欄を足す以外に直す方法が無いため）。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use regex::Regex;
use std::sync::LazyLock;

pub struct DelegateRequestFields;

/// 定型の欄の名前（SKILL.md の定型の見出しと同じ順）
const FIELDS: [&str; 6] = [
    "リポジトリ",
    "ブランチ",
    "何をするか",
    "前提",
    "完成の形",
    "変える範囲",
];

/// `/delegate` の語。前後が英数字やパスの文字なら別の語（`/delegated`、`skills/delegate/SKILL.md`）とする。
/// 日本語の文字は前後に来てよいので `\w`（Unicode）でなく ASCII の文字で区切る
static DELEGATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^A-Za-z0-9_./-])/delegate(?:$|[^A-Za-z0-9_-])").unwrap());

static POINTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"依頼文は\s*`?(/[^\s`]+)`?\s*にある").unwrap());

static HEADINGS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    FIELDS
        .iter()
        .map(|f| Regex::new(&format!(r"(?m)^{f}(?:（[^）\n]*）)?:")).unwrap())
        .collect()
});

impl Rule for DelegateRequestFields {
    fn name(&self) -> &'static str {
        "delegate-request-fields"
    }

    fn tool(&self) -> &'static str {
        "Agent"
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        if !DELEGATE.is_match(&input.prompt) {
            return Vec::new();
        }
        // place は欠けた欄を書いた場所、target は欄を足す先（拒否の文言に使う）
        let (request, place, target) = match POINTER.captures(&input.prompt) {
            Some(c) => {
                let path = input.cwd.join(&c[1]);
                match std::fs::read_to_string(&path) {
                    Ok(text) => (
                        text,
                        format!("依頼文のファイル {}", path.display()),
                        "ファイル",
                    ),
                    Err(e) => {
                        return vec![Finding::Deny(format!(
                            "/delegate の依頼文のファイル {} を読めません（{e}）。欄がそろっているかを判定できないので止めました。\nファイルを書いてから呼び直すか、prompt の「依頼文は <絶対パス> にある」のパスを直してください。",
                            path.display()
                        ))];
                    }
                }
            }
            None => (
                input.prompt.clone(),
                "Agent の prompt".to_string(),
                "prompt",
            ),
        };
        let missing: Vec<&str> = FIELDS
            .iter()
            .zip(HEADINGS.iter())
            .filter(|(_, heading)| !heading.is_match(&request))
            .map(|(f, _)| *f)
            .collect();
        if missing.is_empty() {
            return Vec::new();
        }
        vec![Finding::Deny(format!(
            "/delegate を呼ぶ依頼に定型の欄がそろっていません（{place}）。欠けた欄: {}\n\
             依頼は 6 つの欄（{}）を見出しにして書きます。事実を「前提」の欄に 1 行ずつ出典とともに分けると、書く側は出典を開いたかを見返せ、受ける側は欄を順に確かめられます（~/.claude/skills/delegate/SKILL.md の「頼む側（親のセッション）の書き方」）。\n\
             判定: 行頭が欄の名前で始まり、直後に「:」か「（…）:」が続く行を見出しとします（例: `前提:`、`前提（確かめていない）:`）。箇条書きの中や、コロンの無い行は数えません。事実を使わない依頼は `前提: なし` と書きます。\n\
             修正方法: 欠けた欄を見出しつきで{}に足して、Agent を呼び直してください。依頼をファイルに置くときは、prompt に「依頼文は <絶対パス> にある」と書くと、そのファイルを判定します。",
            missing.join("、"),
            FIELDS.join("、"),
            target,
        ))]
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

    fn check(prompt: &str) -> Vec<Finding> {
        let raw = serde_json::json!({
            "tool_name": "Agent",
            "cwd": "/",
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

    /// テストの間だけある一時のディレクトリ（テストが panic しても消す）
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-delegate-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pointer(path: &std::path::Path) -> String {
        format!(
            "依頼文は {} にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼んで、その手順に従って進める。",
            path.display()
        )
    }

    #[test]
    fn request_with_all_fields_passes() {
        assert!(check(COMPLETE).is_empty());
    }

    #[test]
    fn premise_heading_may_carry_a_parenthesized_qualifier() {
        let prompt = COMPLETE.replace(
            "前提:\n",
            "前提（出典で確かめたもの）:\n- a（b）\n前提（確かめていない。確かめることが作業の一部）:\n",
        );
        assert!(check(&prompt).is_empty());
    }

    #[test]
    fn missing_fields_are_denied_by_name() {
        let prompt = COMPLETE
            .replace("前提:\n- y は z（docs/a.md）\n", "")
            .replace("完成の形: テストが通る\n", "");
        let findings = check(&prompt);
        let reason = deny_reason(&findings);
        assert!(reason.contains("欠けた欄: 前提、完成の形"), "{reason}");
    }

    #[test]
    fn heading_must_start_the_line_and_be_followed_by_a_colon() {
        // 箇条書きの中の語や、コロンの無い語は見出しとして数えない
        let prompt = COMPLETE.replace("前提:\n", "- 前提: なし\n前提は下のとおり\n");
        let findings = check(&prompt);
        assert!(deny_reason(&findings).contains("欠けた欄: 前提"));
    }

    #[test]
    fn prompt_without_delegate_is_not_checked() {
        for prompt in [
            "/code-review high /home/u/repo-feat-x",
            "Explore the hooks crate and report where tool_name is read",
            "/delegated の語や ~/.claude/skills/delegate/SKILL.md のパスを読む依頼は対象外",
        ] {
            assert!(check(prompt).is_empty(), "{prompt}");
        }
    }

    #[test]
    fn prompt_that_only_mentions_delegate_is_not_checked() {
        // /delegate を呼ぶ依頼は 1 行目に「/delegate を呼び」を書く。2 行目以降の引用や、呼ばない文は対象外
        for prompt in [
            "/delegate の手順がどこで前提の欄を確かめるか調べて",
            "/delegate を呼ばずに、この worktree で直接直す",
            "次の差分をレビューする\n+最初に /delegate を呼び、その手順に従って進める。",
        ] {
            assert!(check(prompt).is_empty(), "{prompt}");
        }
    }

    #[test]
    fn request_file_whose_first_line_calls_delegate_is_checked() {
        let tmp = TempDir::new("file-first-line");
        let path = tmp.0.join("brief.md");
        std::fs::write(&path, COMPLETE.replace("完成の形: テストが通る\n", "")).unwrap();
        let prompt = format!("依頼文は {} にある。Read で読んで従う。", path.display());
        assert!(deny_reason(&check(&prompt)).contains("欠けた欄: 完成の形"));
    }

    #[test]
    fn request_file_that_does_not_call_delegate_is_not_checked() {
        let tmp = TempDir::new("file-other");
        let path = tmp.0.join("brief.md");
        std::fs::write(&path, "/retro を回す。\n").unwrap();
        let prompt = format!("依頼文は {} にある。Read で読んで従う。", path.display());
        assert!(check(&prompt).is_empty());
    }

    #[test]
    fn delegate_request_without_any_field_lists_all_six() {
        let findings = check("最初に /delegate を呼ぶ。x を直す。");
        let reason = deny_reason(&findings);
        assert!(
            reason
                .contains("欠けた欄: リポジトリ、ブランチ、何をするか、前提、完成の形、変える範囲"),
            "{reason}"
        );
    }

    #[test]
    fn request_file_named_by_the_prompt_is_checked() {
        let tmp = TempDir::new("file");
        let dir = &tmp.0;
        let complete = dir.join("brief-complete.md");
        std::fs::write(&complete, COMPLETE).unwrap();
        assert!(check(&pointer(&complete)).is_empty());

        let partial = dir.join("brief-partial.md");
        std::fs::write(&partial, COMPLETE.replace("変える範囲: src/\n", "")).unwrap();
        let findings = check(&pointer(&partial));
        let reason = deny_reason(&findings);
        assert!(reason.contains("欠けた欄: 変える範囲"), "{reason}");
        assert!(reason.contains(&partial.display().to_string()), "{reason}");
    }

    #[test]
    fn request_file_in_backquotes_is_read() {
        let tmp = TempDir::new("backquote");
        let dir = &tmp.0;
        let path = dir.join("brief.md");
        std::fs::write(&path, COMPLETE).unwrap();
        let prompt = format!(
            "依頼文は `{}` にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼ぶ。",
            path.display()
        );
        assert!(check(&prompt).is_empty());
    }

    #[test]
    fn missing_request_file_is_denied_with_its_path() {
        let tmp = TempDir::new("absent");
        let dir = &tmp.0;
        let path = dir.join("no-such-brief.md");
        let findings = check(&pointer(&path));
        let reason = deny_reason(&findings);
        assert!(reason.contains(&path.display().to_string()), "{reason}");
        assert!(reason.contains("読めません"), "{reason}");
    }
}
