//! check-merge-review: 既定ブランチへのローカルの git merge の本文に、最後にレビューの周を回した HEAD と指摘ごとの行を
//! 求め、その HEAD より後のコミットが挙動を変えないものだけかを確かめる
//!
//! GitHub の PR はローカルレビューの記録（PR のコメント）を pre-merge-check が head と照らすが、ローカルのマージには
//! PR が無い。記録をマージの本文に置き、履歴に残す（ADR-0070）。本文の形:
//!
//! - `Reviewed: <hash>` の行がちょうど 1 つ（7〜40 桁の小文字の 16 進。最後のレビューの周を回した HEAD）
//! - 指摘ごとの行（`再現: …`、`再現なし → #<番号>…`、`前提が誤り: …`、`抜け道: …`）か `指摘なし` の行が 1 つ以上
//!
//! 行の先頭の空白と箇条の印（`- `、`* `）は除いて読む。本文の決まらない形（エディタ、シェルが展開する値、読めない
//! `-F`）は止める（check-merge-body-ja-terms と違い、後ろに同じ検査をする git の hook が無い）。
//!
//! その hash がブランチの先端の祖先で、既定ブランチの祖先でないことを求め、hash から先端までに変わったファイルが
//! どれも挙動を変えないもの（スキルと hook の文書と ADR を除く `.md`、`dvc.lock` と `.dvc`、hash の付け替えだけの
//! 変更）でなければ止める。後者だけ `ALLOW_UNREVIEWED_HEAD=<理由>` で外せる（形の誤りは外さない）。
//! 対象と中断・再開の扱いは guard-git-merge と同じ

use super::check_merge_body_ja_terms::{Undecided, merge_message};
use super::pre_git_merge_check::{default_branch_merges, local_branch_ref};
use super::{Rule, apply_escape, escape_hint};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::Path;

pub struct CheckMergeReview;

/// 最後の周より後の未レビューの変更を通すエスケープ。GitHub の pre-merge-check と同じ名前
const ESCAPE: &str = "ALLOW_UNREVIEWED_HEAD";

/// deny の理由に示す、挙動を変えるファイルの先頭の件数
const SHOWN_FILES: usize = 10;

/// 本文に書く形。拒否の文言が示す
const FORM: &str = "本文に次の行を書いてください（行の先頭の空白と `- ` は付けてよい）:\n\
  Reviewed: <最後にレビューの周を回した HEAD の hash（7〜40 桁）>\n\
  指摘ごとに 1 行: 再現: <テスト名 | 手順 | 出典> / 再現なし → #<issue の番号> / 前提が誤り: <確かめた事実> / 抜け道: <理由>\n\
  どの周にも指摘が無ければ: 指摘なし\n\
どの head をレビューし、指摘をどう分けたかをマージの履歴に残し、その head より後に挙動を変えるコミットが無いかを確かめるためです。\
レビューを回していなければ、先に /git-merge の「レビュー」の節のとおり回してください";

/// 行の先頭の空白と箇条の印を除いた本体
fn line_body(line: &str) -> &str {
    let t = line.trim();
    t.strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .unwrap_or(t)
        .trim_start()
}

/// 指摘ごとの行か
fn is_finding_line(line: &str) -> bool {
    let has_text = |rest: &str| !rest.trim().is_empty();
    if let Some(rest) = line.strip_prefix("再現なし → #") {
        return rest.starts_with(|c: char| c.is_ascii_digit());
    }
    ["再現:", "前提が誤り:", "抜け道:"]
        .iter()
        .any(|p| line.strip_prefix(p).is_some_and(has_text))
        || line.starts_with("指摘なし")
}

fn is_hash(s: &str) -> bool {
    (7..=40).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// 本文の `Reviewed:` の hash。形を満たさなければ deny の理由
fn reviewed_hash(body: &str) -> Result<String, String> {
    let lines: Vec<&str> = body.lines().map(line_body).collect();
    let reviewed: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("Reviewed:"))
        .map(str::trim)
        .collect();
    let mut missing = Vec::new();
    let hash = match reviewed.as_slice() {
        [h] if is_hash(h) => Some(h.to_string()),
        [h] => {
            missing.push(format!(
                "Reviewed: の値 `{h}` がコミットの hash（7〜40 桁の小文字の 16 進）ではありません"
            ));
            None
        }
        [] => {
            missing.push("Reviewed: の行がありません".to_string());
            None
        }
        _ => {
            missing.push(format!(
                "Reviewed: の行が {} つあります（最後の周の HEAD を 1 つだけ書きます）",
                reviewed.len()
            ));
            None
        }
    };
    if !lines.iter().any(|l| is_finding_line(l)) {
        missing.push("指摘ごとの行（再現: / 再現なし → # / 前提が誤り: / 抜け道:）も 指摘なし の行もありません".to_string());
    }
    match hash {
        Some(h) if missing.is_empty() => Ok(h),
        _ => Err(format!(
            "git merge の本文にレビューの記録が足りません:\n{}\n{FORM}",
            missing
                .iter()
                .map(|m| format!("  - {m}"))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_lower_hex(c: char) -> bool {
    c.is_ascii_digit() || ('a'..='f').contains(&c)
}

/// 16 進の語（コミットの hash）を印に置き換えた行。語は、前後が英数字と `_` でなく、a〜f を 1 字以上含む 7〜64 桁の
/// 小文字の 16 進。数字だけの語は数と見分けられないので置き換えない（数の変更を hash の付け替えに数えない）
fn mask_hashes(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if is_lower_hex(chars[i]) && (i == 0 || !is_word(chars[i - 1])) {
            let mut j = i;
            while j < chars.len() && is_lower_hex(chars[j]) {
                j += 1;
            }
            let run = &chars[i..j];
            let whole = j == chars.len() || !is_word(chars[j]);
            if whole && (7..=64).contains(&run.len()) && run.iter().any(|c| c.is_ascii_alphabetic())
            {
                out.push('\0');
            } else {
                out.extend(run);
            }
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// hunk の消した行と足した行が同じ数で、対の行が 16 進の語を除いて同じか
fn paired_hash_change(minus: &[&str], plus: &[&str]) -> bool {
    !minus.is_empty()
        && minus.len() == plus.len()
        && minus
            .iter()
            .zip(plus)
            .all(|(m, p)| mask_hashes(m) == mask_hashes(p))
}

/// `git diff -U0` の出力（1 ファイル分）が、hunk の対の行で 16 進の語だけを変えたものか（`commit-refs remap` の
/// 付け替え）。hunk の無い変更（モードだけ、バイナリ）は含めない
fn only_hash_changes(diff: &str) -> bool {
    let (mut minus, mut plus) = (Vec::new(), Vec::new());
    let mut hunks = 0;
    for line in diff.lines() {
        if line.starts_with("@@") {
            if hunks > 0 && !paired_hash_change(&minus, &plus) {
                return false;
            }
            minus.clear();
            plus.clear();
            hunks += 1;
        } else if hunks == 0 || line.starts_with('\\') {
            // 最初の hunk の前はファイルの見出し（`--- a/x` も含む）。`\ No newline at end of file` は行でない
        } else if let Some(r) = line.strip_prefix('-') {
            minus.push(r);
        } else if let Some(r) = line.strip_prefix('+') {
            plus.push(r);
        } else {
            return false;
        }
    }
    hunks > 0 && paired_hash_change(&minus, &plus)
}

/// レビューの後に変えても挙動を変えない文書か。スキルと hook の文書（`.claude` の下、`CLAUDE.md`、`SKILL.md`）と
/// ADR（マージの前の accepted への変更もレビューを受ける）は挙動に数える
fn is_plain_document(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.ends_with(".md")
        && !path.split('/').any(|c| c == ".claude")
        && name != "CLAUDE.md"
        && name != "SKILL.md"
        && !path.starts_with("docs/adr/")
}

/// rebase の後に書き直す DVC の記録か（計算の入力の dvc.yaml は含めない）
fn is_dvc_record(path: &str) -> bool {
    path.rsplit('/').next() == Some("dvc.lock") || path.ends_with(".dvc")
}

/// from から to までに変わったファイルのうち、挙動を変えうるもの。git diff が失敗したら None
fn behavior_changes(dir: &Path, from: &str, to: &str) -> Option<Vec<String>> {
    let names = git::git(
        dir,
        &[
            "diff",
            "--name-only",
            "-z",
            // 改名は消したファイルと足したファイルの両方で判定する（コードの改名を文書の名前で通さない）
            "--no-renames",
            "--no-relative",
            from,
            to,
        ],
    )?;
    let mut out = Vec::new();
    for path in names.split('\0').filter(|p| !p.is_empty()) {
        if is_plain_document(path) || is_dvc_record(path) {
            continue;
        }
        let diff = git::git(
            dir,
            &[
                "diff",
                "-U0",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--no-relative",
                from,
                to,
                "--",
                &format!(":(top,literal){path}"),
            ],
        )?;
        if !only_hash_changes(&diff) {
            out.push(path.to_string());
        }
    }
    Some(out)
}

fn is_ancestor(dir: &Path, a: &str, b: &str) -> bool {
    git::git(dir, &["merge-base", "--is-ancestor", a, b]).is_some()
}

fn resolve_commit(dir: &Path, spec: &str) -> Option<String> {
    git::git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{spec}^{{commit}}"),
        ],
    )
}

/// 本文が決まらないときの deny の理由
fn undecided_reason(why: Option<Undecided>) -> String {
    let what = match why {
        None => "本文が -m / -F で渡されていない".to_string(),
        Some(Undecided::Expanded(opt)) => format!("{opt} の値をシェルが展開する"),
        Some(Undecided::Unreadable(p)) => format!("-F {p} を読めない"),
    };
    format!(
        "git merge の本文を読めないので、レビューの記録を確かめられません（{what}）。本文を書いたまま -m '…'、\
         クォートした区切りのヒアドキュメント（-m \"$(cat <<'EOF' … EOF)\"）か、読めるファイルの -F で渡してください。\n{FORM}"
    )
}

/// 1 本のブランチのマージを確かめる。(止める理由, エスケープで外せる止める理由)
fn check_branch(
    dir: &Path,
    default: &str,
    branch: &str,
    full: &str,
    body: &str,
) -> (Option<String>, Option<String>) {
    let hash = match reviewed_hash(body) {
        Ok(h) => h,
        Err(r) => return (Some(r), None),
    };
    let Some(reviewed) = resolve_commit(dir, &hash) else {
        return (
            Some(format!(
                "Reviewed: の {hash} がコミットに解けません（無いか、短くて曖昧）。最後にレビューの周を回した HEAD の hash を書いてください"
            )),
            None,
        );
    };
    let Some(tip) = resolve_commit(dir, full) else {
        return (Some(format!("{branch} の先端を読めません")), None);
    };
    if !is_ancestor(dir, &reviewed, &tip) {
        return (
            Some(format!(
                "Reviewed: の {hash} は {branch} の先端の祖先ではありません。rebase で書き換わったなら、\
                 git range-diff <古い土台>..<旧先端> {default}..{branch} で対応するコミットを確かめ、その rebase 後の hash を書いてください。\
                 対応が取れなければ、ブランチ全体をレビューし直してください"
            )),
            None,
        );
    }
    if is_ancestor(dir, &reviewed, default) {
        return (
            Some(format!(
                "Reviewed: の {hash} は {default} にあるコミットで、{branch} のブランチのコミットではありません。最後にレビューの周を回した {branch} の HEAD を書いてください"
            )),
            None,
        );
    }
    if reviewed == tip {
        return (None, None);
    }
    let Some(changed) = behavior_changes(dir, &reviewed, &tip) else {
        return (
            Some(format!(
                "{hash} から {branch} の先端までの変更を読めません（git diff が失敗）"
            )),
            None,
        );
    };
    if changed.is_empty() {
        return (None, None);
    }
    let shown: Vec<String> = changed
        .iter()
        .take(SHOWN_FILES)
        .map(|p| format!("  {p}"))
        .collect();
    let more = match changed.len().saturating_sub(SHOWN_FILES) {
        0 => String::new(),
        n => format!("\n  …ほか {n} 件"),
    };
    (
        None,
        Some(format!(
            "Reviewed: の {hash} より後に、{branch} が挙動を変えうるファイルを変えています（{} 件）:\n{}{more}\n\
             レビューの後に通せるのは、スキルと hook の文書と ADR を除く .md、dvc.lock と .dvc、コミットの hash の付け替えだけです。\
             /code-review をブランチ全体に回し直して Reviewed: にその HEAD を書くか、挙動を変えるコミットをブランチから外してください。{}",
            changed.len(),
            shown.join("\n"),
            escape_hint(ESCAPE)
        )),
    )
}

impl Rule for CheckMergeReview {
    fn name(&self) -> &'static str {
        "check-merge-review"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut out = Vec::new();
        let mut unreviewed = Vec::new();
        for merge in default_branch_merges(input, shell) {
            let branches: Vec<(&str, String)> = merge
                .targets
                .iter()
                .filter_map(|t| local_branch_ref(&merge.dir, t).map(|full| (t.as_str(), full)))
                .collect();
            let [(name, full)] = branches.as_slice() else {
                if branches.len() > 1 {
                    out.push(Finding::Deny(format!(
                        "{} 本のブランチを 1 回でマージしています。本文の Reviewed: をどのブランチに当てるか決まらないので、1 本ずつマージしてください",
                        branches.len()
                    )));
                }
                continue;
            };
            let body = match merge_message(&merge.args, &merge.dir) {
                Ok(Some(b)) => b,
                Ok(None) => {
                    out.push(Finding::Deny(undecided_reason(None)));
                    continue;
                }
                Err(why) => {
                    out.push(Finding::Deny(undecided_reason(Some(why))));
                    continue;
                }
            };
            let branch = full.strip_prefix("refs/heads/").unwrap_or(name);
            let (deny, escapable) = check_branch(&merge.dir, &merge.default, branch, full, &body);
            out.extend(deny.map(Finding::Deny));
            unreviewed.extend(escapable.map(Finding::Deny));
        }
        out.extend(apply_escape(shell.escape(ESCAPE), ESCAPE, unreviewed));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// テストの間だけある一時のディレクトリ（テストが panic しても消す）。テストは並行に走るので、呼ぶたびに別のディレクトリにする
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-merge-review-{tag}-{}-{seq}",
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

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(dir: &Path, args: &[&str]) {
        git_out(dir, args);
    }

    /// main に 1 コミット、feat/x に main から 1 コミット（src/a.rs）を持つリポジトリ。今のブランチは main。
    /// git の hook（手元のグローバルの core.hooksPath）は実行しない
    struct Repo {
        root: TempDir,
    }

    impl Repo {
        fn new(tag: &str) -> Repo {
            let root = TempDir::new(tag);
            let repo = Repo { root };
            let dir = repo.dir();
            std::fs::create_dir_all(&dir).unwrap();
            git(&dir, &["init", "-q", "-b", "main"]);
            for (k, v) in [
                ("user.name", "t"),
                ("user.email", "t@example.com"),
                ("core.hooksPath", "/nonexistent/hooks"),
                ("commit.gpgsign", "false"),
            ] {
                git(&dir, &["config", k, v]);
            }
            repo.write("README.md", "readme\n");
            repo.commit("init");
            git(&dir, &["switch", "-q", "-c", "feat/x"]);
            repo.write("src/a.rs", "fn a() {}\n");
            repo.commit("feat: a");
            git(&dir, &["switch", "-q", "main"]);
            repo
        }

        fn dir(&self) -> PathBuf {
            self.root.0.join("repo")
        }

        fn write(&self, path: &str, text: &str) {
            let p = self.dir().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }

        fn commit(&self, msg: &str) -> String {
            git(&self.dir(), &["add", "-A"]);
            git(&self.dir(), &["commit", "-q", "-m", msg]);
            git_out(&self.dir(), &["rev-parse", "HEAD"])
        }

        /// feat/x に移ってファイルを書き、コミットして main に戻る。コミットの hash
        fn commit_on_branch(&self, files: &[(&str, &str)]) -> String {
            git(&self.dir(), &["switch", "-q", "feat/x"]);
            for (p, t) in files {
                self.write(p, t);
            }
            let h = self.commit("change");
            git(&self.dir(), &["switch", "-q", "main"]);
            h
        }

        fn tip(&self) -> String {
            git_out(&self.dir(), &["rev-parse", "feat/x"])
        }

        /// 本文を -F のファイル（リポジトリの外）で渡して判定する
        fn check_body(&self, body: &str) -> Vec<Finding> {
            let msg = self.root.0.join("msg.txt");
            std::fs::write(&msg, body).unwrap();
            self.check(&format!(
                "git merge --no-ff feat/x -F {}",
                msg.to_string_lossy()
            ))
        }

        fn check(&self, command: &str) -> Vec<Finding> {
            let json = serde_json::json!({
                "tool_name": "Bash",
                "tool_input": {"command": command},
                "cwd": self.dir(),
            });
            let input = Input::parse(&json.to_string()).unwrap();
            CheckMergeReview.check(&input, &Shell::parse(command))
        }
    }

    fn body(reviewed: &str, findings: &str) -> String {
        format!(
            "Merge: x\n\n## Why\nw\n\n## Impact\n{findings}\n\nReviewed: {reviewed}\nBranch: feat/x\n"
        )
    }

    const FINDINGS: &str = "- 再現: `a_test` — 境界の誤り\n- 再現なし → #12: 効率";

    fn denies(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .filter_map(|f| match f {
                Finding::Deny(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }

    fn only_deny(findings: &[Finding]) -> String {
        match denies(findings).as_slice() {
            [r] => r.clone(),
            other => panic!("deny が 1 件のはず: {other:?}"),
        }
    }

    #[test]
    fn passes_when_the_reviewed_head_is_the_tip() {
        let repo = Repo::new("tip");
        let tip = repo.tip();
        assert!(denies(&repo.check_body(&body(&tip, FINDINGS))).is_empty());
        // 短い hash も解く
        assert!(denies(&repo.check_body(&body(&tip[..9], FINDINGS))).is_empty());
    }

    #[test]
    fn passes_documents_dvc_records_and_remap_after_the_reviewed_head() {
        let repo = Repo::new("after-ok");
        let reviewed = repo.commit_on_branch(&[("src/b.rs", "// see 0123abcd4\n")]);
        repo.commit_on_branch(&[
            ("docs/notes.md", "notes\n"),
            ("docs/issues/0001-x.md", "issue\n"),
            ("examples/a/dvc.lock", "outs: 1\n"),
            ("examples/a/out.dvc", "md5: 1\n"),
            ("src/b.rs", "// see 9876fedc1\n"),
        ]);
        let r = repo.check_body(&body(&reviewed, FINDINGS));
        assert!(denies(&r).is_empty(), "{:?}", denies(&r));
    }

    #[test]
    fn denies_code_and_test_changes_after_the_reviewed_head() {
        let repo = Repo::new("after-code");
        let reviewed = repo.tip();
        repo.commit_on_branch(&[("src/a.rs", "fn a() { b() }\n"), ("docs/n.md", "n\n")]);
        let r = only_deny(&repo.check_body(&body(&reviewed, FINDINGS)));
        assert!(r.contains("src/a.rs"), "{r}");
        assert!(!r.contains("docs/n.md"), "{r}");
        assert!(r.contains("/code-review"), "{r}");
        assert!(r.contains("ALLOW_UNREVIEWED_HEAD=<理由>"), "{r}");
    }

    #[test]
    fn counts_skill_hook_documents_and_adrs_as_behavior() {
        for path in [
            ".claude/skills/a/SKILL.md",
            ".claude/README.md",
            "CLAUDE.md",
            "sub/CLAUDE.md",
            "skills/b/SKILL.md",
            "docs/adr/0001-x.md",
            "examples/a/dvc.yaml",
        ] {
            let repo = Repo::new("after-docs");
            let reviewed = repo.tip();
            repo.commit_on_branch(&[(path, "text\n")]);
            let r = only_deny(&repo.check_body(&body(&reviewed, FINDINGS)));
            assert!(r.contains(path), "{path}: {r}");
        }
    }

    #[test]
    fn escape_passes_unreviewed_changes_but_not_a_missing_form() {
        let repo = Repo::new("escape");
        let reviewed = repo.tip();
        repo.commit_on_branch(&[("src/a.rs", "fn a() { b() }\n")]);
        let msg = repo.root.0.join("msg.txt");
        std::fs::write(&msg, body(&reviewed, FINDINGS)).unwrap();
        let cmd = format!(
            "ALLOW_UNREVIEWED_HEAD='歯止めの 3 周を使い切った後の変更で、Impact に未レビューと書いた' git merge --no-ff feat/x -F {}",
            msg.to_string_lossy()
        );
        assert!(denies(&repo.check(&cmd)).is_empty());
        // 値が短ければ理由を求める
        let short = format!(
            "ALLOW_UNREVIEWED_HEAD=1 git merge --no-ff feat/x -F {}",
            msg.to_string_lossy()
        );
        let d = denies(&repo.check(&short));
        assert!(d.iter().any(|r| r.contains("理由を記載")), "{d:?}");
        // 形の誤りはエスケープで外れない
        std::fs::write(&msg, body(&reviewed, "")).unwrap();
        assert_eq!(denies(&repo.check(&cmd)).len(), 1);
    }

    #[test]
    fn denies_a_body_without_one_reviewed_line() {
        let repo = Repo::new("form");
        let tip = repo.tip();
        let none = only_deny(&repo.check_body(&format!("Merge: x\n\n{FINDINGS}\n")));
        assert!(none.contains("Reviewed: <"), "{none}");
        let two =
            only_deny(&repo.check_body(&format!("{}Reviewed: {tip}\n", body(&tip, FINDINGS))));
        assert!(two.contains("Reviewed:"), "{two}");
        for bad in ["", "abc", "HEAD", "ABCDEF1234", "feat/x"] {
            let r = only_deny(&repo.check_body(&body(bad, FINDINGS)));
            assert!(r.contains("Reviewed:"), "{bad}: {r}");
        }
    }

    #[test]
    fn denies_a_body_without_finding_lines() {
        let repo = Repo::new("findings");
        let tip = repo.tip();
        for findings in [
            "",
            "レビューは 3 周（再現: `a_test`）",
            "- 再現:",
            "- 再現なし → 効率",
            "- 前提が誤り:",
        ] {
            let r = only_deny(&repo.check_body(&body(&tip, findings)));
            assert!(r.contains("再現なし → #"), "{findings}: {r}");
            assert!(r.contains("指摘なし"), "{findings}: {r}");
        }
    }

    #[test]
    fn accepts_each_finding_form() {
        let repo = Repo::new("forms");
        let tip = repo.tip();
        for findings in [
            "再現: `a_test`",
            "- 再現なし → #0190: 効率",
            "* 前提が誤り: 関数は既に検査している",
            "  - 抜け道: 作為的な書き方",
            "指摘なし",
            "- 指摘なし（high で 1 周）",
        ] {
            let r = repo.check_body(&body(&tip, findings));
            assert!(denies(&r).is_empty(), "{findings}: {:?}", denies(&r));
        }
    }

    #[test]
    fn denies_a_reviewed_hash_off_the_branch() {
        let repo = Repo::new("off");
        // rebase で書き換わったときと同じく、ブランチの先端の祖先でないコミット
        git(&repo.dir(), &["switch", "-q", "-c", "other"]);
        repo.write("src/c.rs", "c\n");
        let other = repo.commit("other");
        git(&repo.dir(), &["switch", "-q", "main"]);
        let r = only_deny(&repo.check_body(&body(&other, FINDINGS)));
        assert!(r.contains("range-diff"), "{r}");
        // 既定ブランチのコミット
        let main = git_out(&repo.dir(), &["rev-parse", "main"]);
        let r = only_deny(&repo.check_body(&body(&main, FINDINGS)));
        assert!(r.contains("ブランチのコミット"), "{r}");
        // コミットに解けない
        let r = only_deny(&repo.check_body(&body("0000000deadbeef", FINDINGS)));
        assert!(r.contains("0000000deadbeef"), "{r}");
    }

    #[test]
    fn denies_a_merge_whose_body_is_not_known() {
        let repo = Repo::new("nobody");
        for cmd in [
            "git merge --no-ff feat/x",
            "git merge --no-ff feat/x -m \"$MSG\"",
            "git merge --no-ff feat/x -F /nonexistent/msg.txt",
        ] {
            let r = only_deny(&repo.check(cmd));
            assert!(r.contains("本文"), "{cmd}: {r}");
        }
    }

    #[test]
    fn reads_a_heredoc_body() {
        let repo = Repo::new("heredoc");
        let tip = repo.tip();
        let cmd = format!(
            "git merge --no-ff feat/x -m \"$(cat <<'EOF'\n{}EOF\n)\"",
            body(&tip, FINDINGS)
        );
        assert!(denies(&repo.check(&cmd)).is_empty());
    }

    #[test]
    fn denies_an_octopus_merge() {
        let repo = Repo::new("octopus");
        git(&repo.dir(), &["branch", "feat/y", "feat/x"]);
        let tip = repo.tip();
        let msg = repo.root.0.join("msg.txt");
        std::fs::write(&msg, body(&tip, FINDINGS)).unwrap();
        let r = only_deny(&repo.check(&format!(
            "git merge --no-ff feat/x feat/y -F {}",
            msg.to_string_lossy()
        )));
        assert!(r.contains("1 本ずつ"), "{r}");
    }

    #[test]
    fn ignores_merges_outside_the_default_branch_and_merge_control() {
        let repo = Repo::new("ignore");
        for cmd in [
            "git merge --abort",
            "git merge --continue",
            "git switch feat/x && git merge main",
        ] {
            assert!(denies(&repo.check(cmd)).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn masks_only_whole_hex_words_with_a_letter() {
        assert_eq!(
            mask_hashes("see b7e01d512。"),
            mask_hashes("see 0123abcd4。")
        );
        // 数字だけの語は数の変更と見分けられない
        assert_ne!(mask_hashes("k = 1234567"), mask_hashes("k = 1234568"));
        // 語の一部は置き換えない
        assert_ne!(mask_hashes("xabcdef12"), mask_hashes("xabcdef13"));
        assert_ne!(mask_hashes("abc12"), mask_hashes("abc13"));
    }

    #[test]
    fn hash_only_diff_needs_paired_hunks() {
        let remap = "diff --git a/r.md b/r.md\n--- a/r.md\n+++ b/r.md\n@@ -3 +3 @@\n-at b7e01d512\n+at 0123abcd4\n";
        assert!(only_hash_changes(remap));
        let text = "--- a/r.md\n+++ b/r.md\n@@ -3 +3 @@\n-at b7e01d512\n+on 0123abcd4\n";
        assert!(!only_hash_changes(text));
        let added = "--- a/r.md\n+++ b/r.md\n@@ -3,0 +4 @@\n+at 0123abcd4\n";
        assert!(!only_hash_changes(added));
        // hunk の無い変更（モードだけ、バイナリ）
        assert!(!only_hash_changes(
            "diff --git a/x b/x\nold mode 100644\nnew mode 100755\n"
        ));
        assert!(!only_hash_changes(""));
    }
}
