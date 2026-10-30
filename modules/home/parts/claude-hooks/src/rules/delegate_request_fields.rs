//! delegate-request-fields: /delegate を呼ぶ Agent の依頼に、定型の 6 つの欄がそろっているかを確かめる
//!
//! 依頼の定型（~/.claude/skills/delegate/SKILL.md の「頼む側（親のセッション）の書き方」）は、事実を「前提」の欄に
//! 1 行ずつ分けて書かせる。欄が欠けると、書く側は出典を開いたかを見返せず、受ける側は本文から事実を拾い出す。
//! 欄の有無は依頼の文字列だけで決まるので、送る前にここで判定する。
//!
//! - 依頼の置き場: prompt の空でない最初の行に「依頼文は <絶対パス> にある」（パスは `` ` `` か「」で囲んでもよい）が
//!   あれば、そのファイル。無ければ prompt。2 行目以降の同じ句は、ほかの依頼文への言及として扱う
//! - 対象: prompt か依頼のファイルの、空でない最初の行に「/delegate を呼び、」（呼ぶ。、呼んで、）がある呼び出し。定型の
//!   1 行目がこの句なので、/delegate に触れるだけの prompt（/delegate を調べる依頼、差分の引用）は見ない
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

/// /delegate を呼ぶ句。活用の形と、その後の句読点か行末までを見る。「呼ばずに」「呼びません」「呼ぶ必要はない」は
/// 呼ばない文なので、語の途中で切れる形を数えない
static CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/delegate\s*を呼(?:び|ぶ|んで)(?:[、。,.\s]|$)").unwrap());

/// 依頼のファイルを指す句。パスは `/`、`~`、`.` で始まるものだけを見る（「依頼文は下にある」は指していない）。
/// 絶対パスかは句を見つけた後に確かめ、拒否の文言で修正方法を示す
static POINTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"依頼文は\s*[`「]?([/~.][^\s`「」]*?)[`」]?\s*にある").unwrap());

/// 依頼のファイルの大きさの上限。hook は Agent の呼び出しを待たせるので、依頼文より大きいものを読まない
const MAX_REQUEST_BYTES: u64 = 1 << 20;

static HEADINGS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    FIELDS
        .iter()
        .map(|f| Regex::new(&format!(r"(?m)^{f}(?:（[^）\n]*）)?:")).unwrap())
        .collect()
});

fn first_line(text: &str) -> &str {
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or("")
}

/// 空でない最初の行が /delegate を呼ぶ句を持つか
fn calls_delegate(text: &str) -> bool {
    CALL.is_match(first_line(text))
}

/// 依頼のファイルを読む。通常のファイルでないもの（FIFO やデバイス）は、読むと hook が止まるので読まない
fn read_request(path: &str) -> Result<String, String> {
    if !path.starts_with('/') {
        return Err("絶対パスではありません".to_string());
    }
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("通常のファイルではありません".to_string());
    }
    if meta.len() > MAX_REQUEST_BYTES {
        return Err(format!("{MAX_REQUEST_BYTES} バイトを超えます"));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

impl Rule for DelegateRequestFields {
    fn name(&self) -> &'static str {
        "delegate-request-fields"
    }

    fn tool(&self) -> &'static str {
        "Agent"
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        let prompt_calls = calls_delegate(&input.prompt);
        // place は欠けた欄を書いた場所、target は欄を足す先（拒否の文言に使う）
        let (request, place, target) = match POINTER.captures(first_line(&input.prompt)) {
            Some(c) => {
                let path = &c[1];
                match read_request(path) {
                    Ok(text) if prompt_calls || calls_delegate(&text) => {
                        (text, format!("依頼文のファイル {path}"), "ファイル")
                    }
                    Ok(_) => return Vec::new(),
                    Err(e) if prompt_calls => {
                        return vec![Finding::Deny(format!(
                            "/delegate の依頼文のファイル {path} を読めません（{e}）。欄がそろっているかを判定できないので止めました。\nファイルを書いてから呼び直すか、prompt の 1 行目の「依頼文は <絶対パス> にある」のパスを直してください（`~` や相対のパスは使えません）。"
                        ))];
                    }
                    // /delegate を呼ぶかは読めないファイルの側にあるかもしれないが、prompt が呼ばないなら対象と決められない
                    Err(_) => return Vec::new(),
                }
            }
            None if prompt_calls => (
                input.prompt.clone(),
                "Agent の prompt".to_string(),
                "prompt",
            ),
            None => return Vec::new(),
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
             修正方法: 欠けた欄を見出しつきで{}に足して、Agent を呼び直してください。依頼をファイルに置くときは、prompt の 1 行目に「依頼文は <絶対パス> にある」と書くと、そのファイルを判定します。",
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
    fn premise_qualifier_may_nest_parentheses() {
        // 限定の中に出典の括弧を入れた見出し（2026-10-30 の xlc の依頼文の形）
        let prompt = COMPLETE.replace(
            "前提:\n",
            "前提（コードの行は main abc1234。親が主な行（`HistoryFile` 688 行目）を確かめた）:\n",
        );
        assert!(check(&prompt).is_empty());
    }

    #[test]
    fn unbalanced_qualifier_is_not_a_heading() {
        // 括弧が閉じきらない、または閉じた直後にコロンが無い行は見出しにしない
        for heading in ["前提（a（b）:\n", "前提（a）は b（c）:\n"] {
            let prompt = COMPLETE.replace("前提:\n", heading);
            let findings = check(&prompt);
            assert!(
                deny_reason(&findings).contains("欠けた欄: 前提"),
                "{heading}"
            );
        }
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
            "/delegate を呼ぶ必要はない。この worktree で直接直す",
            "/delegate を呼びません。",
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

    #[test]
    fn inline_request_citing_another_brief_is_judged_by_the_prompt() {
        // 依頼のファイルを指すのは 1 行目の句だけ。前提の欄で別の依頼文に触れても、prompt を判定する
        let prompt = COMPLETE.replace(
            "- y は z（docs/a.md）\n",
            "- 前の依頼文は /nonexistent/brief-old.md にある（読んだ）\n",
        );
        assert!(check(&prompt).is_empty());
    }

    #[test]
    fn request_file_in_corner_brackets_is_read() {
        let tmp = TempDir::new("brackets");
        let path = tmp.0.join("brief.md");
        std::fs::write(&path, COMPLETE).unwrap();
        let prompt = format!(
            "依頼文は「{}」にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼ぶ。",
            path.display()
        );
        assert!(check(&prompt).is_empty());
    }

    #[test]
    fn request_file_given_by_a_relative_path_is_denied_with_the_fix() {
        let prompt = "依頼文は ~/brief.md にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼ぶ。";
        let findings = check(prompt);
        let reason = deny_reason(&findings);
        assert!(reason.contains("~/brief.md"), "{reason}");
        assert!(reason.contains("絶対パス"), "{reason}");
    }

    #[test]
    fn request_file_that_is_not_a_regular_file_is_denied_without_reading() {
        let tmp = TempDir::new("dir");
        let prompt = format!(
            "依頼文は {} にある。最初にそれを Read で読み、冒頭の指示どおり /delegate を呼ぶ。",
            tmp.0.display()
        );
        let findings = check(&prompt);
        assert!(deny_reason(&findings).contains("読めません"));
    }

    #[test]
    fn inline_request_saying_it_is_below_is_judged_by_the_prompt() {
        // 「依頼文は下にある」はパスでないので、ファイルを指す句と見なさない
        let prompt = format!("依頼文は下にある。{COMPLETE}");
        assert!(check(&prompt).is_empty());
    }
}
