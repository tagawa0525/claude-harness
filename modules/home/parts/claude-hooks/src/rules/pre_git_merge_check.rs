//! pre-git-merge-check: 既定ブランチへのローカルの git merge の前に、ブランチのトピックが 1 つかを確かめる
//!
//! GitHub を使わないリポジトリは feature branch を main に `git merge --no-ff` する。
//! gh pr merge の pre-merge-check と同じく、ブランチが新しい ADR（docs/adr/ 直下の
//! NNNN-*.md）を 2 件以上加えていれば deny する。ADR は 1 件 1 決定なので、新しい ADR の数が
//! トピックの数の目安になる。既存の ADR の変更（status の superseded への変更など）は数えない。
//!
//! 対象はローカルのブランチ（refs/heads/）を既定ブランチの上でマージするときだけ。今のブランチは
//! 同じコマンドの前の git switch / checkout を反映し（branch_at）、移り先が分からなければ確かめる。
//! リモート追跡ブランチの取り込み（`git merge origin/main`）は別々にマージ済みのトピックの
//! 集まりなので対象外。docs/adr の無いリポジトリでは数える ADR が無いので何もしない。
//! エスケープ: `ALLOW_MULTI_TOPIC=<理由>`（1 つの決定を複数の ADR に分けて書いたときと、
//! 決定前の案（status: proposed）の ADR をまとめて加えるときだけ）

use super::guard_branch_base::branch_at;
use super::{Rule, escape_hint};
use crate::git;
use crate::git_args::{Item, items};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};
use design_records::is_adr_path;
use design_records::parse::parse_front_matter;
use std::path::{Path, PathBuf};

pub struct PreGitMergeCheck;

/// 新しい ADR が 2 件以上なら deny の理由。2 件以上でも、全部がフロントマターの `requires` で 1 つにつながっていれば
/// 1 つの決定を分けて書いたものなので通す。`read` は ADR のパスから中身を読む（読めない ADR はつなげない）
pub(super) fn multi_topic_reason(
    new_adrs: &[String],
    read: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if new_adrs.len() < 2 || linked_by_requires(new_adrs, &read) {
        return None;
    }
    let list: Vec<String> = new_adrs.iter().map(|a| format!("  - {a}")).collect();
    Some(format!(
        "このブランチは新しい ADR を {} 件加えています（1 ブランチ 1 トピック、ADR は 1 件 1 決定）:\n{}\n\
         決定ごとにブランチを分けてください（/topic-triage）。{}",
        new_adrs.len(),
        list.join("\n"),
        escape_hint("ALLOW_MULTI_TOPIC")
    ))
}

/// ADR のパス（`docs/adr/0031-x.md`）の番号
fn adr_number(path: &str) -> Option<u32> {
    path.rsplit('/').next()?.get(..4)?.parse().ok()
}

/// フロントマターの `requires`（`[ADR-0031]`）が挙げる ADR の番号
fn required_numbers(text: &str) -> Vec<u32> {
    let Ok(front) = parse_front_matter(text) else {
        return Vec::new();
    };
    front
        .get("requires")
        .map(|v| {
            v.items()
                .iter()
                .filter_map(|r| {
                    // design-records の規則: ADR- の後がちょうど 4 桁の ASCII 数字
                    let digits = r.strip_prefix("ADR-")?;
                    (digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()))
                        .then(|| digits.parse().ok())
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 新しい ADR 全部が、新しい ADR どうしの `requires`（向きは問わない）で 1 つにつながっているか。
/// ブランチの外の ADR への `requires` はつなげない
fn linked_by_requires(new_adrs: &[String], read: &impl Fn(&str) -> Option<String>) -> bool {
    let numbers: Vec<Option<u32>> = new_adrs.iter().map(|p| adr_number(p)).collect();
    let mut group: Vec<usize> = (0..new_adrs.len()).collect();
    for (i, path) in new_adrs.iter().enumerate() {
        let Some(text) = read(path) else { continue };
        for required in required_numbers(&text) {
            let Some(j) = numbers.iter().position(|n| *n == Some(required)) else {
                continue;
            };
            let (from, to) = (group[i], group[j]);
            for g in group.iter_mut().filter(|g| **g == from) {
                *g = to;
            }
        }
    }
    group.iter().all(|g| *g == group[0])
}

/// `rev` 時点のファイルの中身を読む（`multi_topic_reason` の `read`）
pub(super) fn read_at<'a>(dir: &'a Path, rev: &'a str) -> impl Fn(&str) -> Option<String> + 'a {
    move |path| git::git(dir, &["show", &format!("{rev}:{path}")])
}

/// `range`（`A...B`）で加わった ADR のパス。git diff が失敗したら None
pub(super) fn added_adrs(dir: &Path, range: &str) -> Option<Vec<String>> {
    // pathspec で docs/adr に絞ると、git は移動元のパスを見ないので、別のディレクトリから移した ADR が
    // 改名でなく追加になる。全体の差分から追加（A）の行を取り、パスで絞る
    let diff = git::git(
        dir,
        &[
            "diff",
            "--find-renames",
            // 出力のパスをリポジトリの直下からにする。diff.relative の設定に左右させない
            "--no-relative",
            "--diff-filter=A",
            "--name-status",
            // パスをクォートせずに NUL 区切りで受け取る（非 ASCII のファイル名）。--diff-filter=A なので
            // 組は `A NUL パス NUL`（改名や複製の 2 パスの組は出ない）
            "-z",
            range,
        ],
    )?;
    let fields: Vec<&str> = diff.split('\0').collect();
    Some(
        fields
            .chunks(2)
            .filter(|c| c[0] == "A")
            .filter_map(|c| c.get(1))
            .filter(|p| is_adr_path(p))
            .map(|p| p.to_string())
            .collect(),
    )
}

/// `git merge args…` のマージ対象。中断・再開（--abort / --continue / --quit）なら None。
/// 対象を標準入力で受け取る `--stdin` はそのまま対象に入れる（check で確かめられないとして止める）
fn merge_targets(args: &[Arg]) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for item in items("merge", args) {
        match item {
            Item::Long("--abort" | "--continue" | "--quit") => return None,
            Item::Long("--stdin") => out.push("--stdin".to_string()),
            Item::Positional(a) => out.push(a.text.clone()),
            _ => {}
        }
    }
    Some(out)
}

/// マージコミットを必ず作る指定か。`--no-ff` があっても、後ろの `--ff` / `--ff-only` / `--squash` が
/// 勝って fast-forward や squash になることがあるので、順序を読まずに併用ごと認めない
fn always_merge_commit(args: &[Arg]) -> bool {
    let longs: Vec<&str> = items("merge", args)
        .into_iter()
        .filter_map(|item| match item {
            Item::Long(name) => Some(name),
            _ => None,
        })
        .collect();
    longs.contains(&"--no-ff")
        && !longs
            .iter()
            .any(|name| matches!(*name, "--ff" | "--ff-only" | "--squash"))
}

/// 既定ブランチの上で行う `git merge` 1 回分
pub(super) struct DefaultBranchMerge {
    pub dir: PathBuf,
    /// ローカルの既定ブランチ名（`main`）
    pub default: String,
    /// マージ対象の指定（ブランチ名、`-`、`--stdin`）
    pub targets: Vec<String>,
    /// `--no-ff` で、fast-forward や squash になる指定がない
    pub no_ff: bool,
    /// `merge` の後の引数（本文の `-m` / `-F` を読む）
    pub args: Vec<Arg>,
}

/// コマンドに含まれる、既定ブランチの上で行う `git merge`。
/// 今のブランチは同じコマンドの前の git switch / checkout を反映し（branch_at）、移り先が分からなければ
/// 既定ブランチとして扱う（確かめる側に倒す）。中断・再開は含めない
pub(super) fn default_branch_merges(input: &Input, shell: &Shell) -> Vec<DefaultBranchMerge> {
    let mut out = Vec::new();
    for cmd in shell.commands() {
        let Some((targets, no_ff, args)) = cmd
            .git_subcommand()
            .filter(|(sub, _)| *sub == "merge")
            .and_then(|(_, args)| {
                merge_targets(args).map(|t| (t, always_merge_commit(args), args.to_vec()))
            })
        else {
            continue;
        };
        let dir = shell.target_dir(cmd, &input.cwd);
        let Some(default_ref) = git::default_branch(&dir) else {
            continue;
        };
        let default = git::local_name(&default_ref).to_string();
        if branch_at(shell, cmd, &input.cwd, &dir).is_some_and(|b| b != default) {
            continue;
        }
        out.push(DefaultBranchMerge {
            dir,
            default,
            targets,
            no_ff,
            args,
        });
    }
    out
}

/// マージ対象の指定をローカルのブランチの完全な ref（`refs/heads/feat/x`）にする。
/// リモート追跡ブランチや解決できない名前は None（解決できない名前は git merge 自身が失敗する）
pub(super) fn local_branch_ref(dir: &Path, target: &str) -> Option<String> {
    let spec = if target == "-" { "@{-1}" } else { target };
    git::git(dir, &["rev-parse", "--symbolic-full-name", spec])
        .filter(|f| f.starts_with("refs/heads/"))
}

/// マージ元の rev。ローカルのブランチは完全な ref、ブランチ名でないコミットの指定（SHA、`feat/x^0`、タグ、`:/文字列`）は
/// コミットの hash にして返す。リモート追跡ブランチと、解決できない名前は None。
/// ブランチ名だけを見ると、`git merge --no-ff <SHA>` が、rebase 済みの確認、ADR の数、本文のレビューの記録の
/// どれにも掛からない。3 つのルール（guard-git-merge、pre-git-merge-check、check-merge-review）が共有する
pub(super) fn merge_source(dir: &Path, target: &str) -> Option<String> {
    if let Some(full) = local_branch_ref(dir, target) {
        return Some(full);
    }
    let spec = if target == "-" { "@{-1}" } else { target };
    // リモート追跡ブランチの取り込みは別々にマージ済みのトピックの集まりなので対象外。タグは指定のまま確かめる
    let is_remote = git::git(dir, &["rev-parse", "--symbolic-full-name", spec])
        .is_some_and(|f| f.starts_with("refs/remotes/"));
    if is_remote {
        return None;
    }
    // 指定のままでなく hash を返す（後の `{rev}:{path}` や `A...B` の組み立てで、`:/文字列` や空白が壊れない）。
    // 注釈つきタグはコミットに剥がす
    commit_ish(dir, spec)
        .and_then(|id| git::git(dir, &["rev-parse", "--verify", &format!("{id}^{{commit}}")]))
}

/// 指定（SHA、`feat/x^0`、タグ、`:/文字列`）が指すコミットの hash。コミットかタグのオブジェクトだけを認め、
/// 解決できなければ None。`^{commit}` を付けて解くと `:/f^{commit}` が検索の文字列の一部になるので、
/// 付けずに解いてオブジェクトの種類で絞る。`$(…)` の展開を含む指定は値が分からないので、確かめない
pub(super) fn commit_ish(dir: &Path, spec: &str) -> Option<String> {
    let id = git::git(dir, &["rev-parse", "--verify", "--quiet", spec])?;
    matches!(
        git::git(dir, &["cat-file", "-t", &id]).as_deref(),
        Some("commit" | "tag")
    )
    .then_some(id)
}

impl Rule for PreGitMergeCheck {
    fn name(&self) -> &'static str {
        "pre-git-merge-check"
    }

    fn escape(&self) -> Option<&'static str> {
        Some("ALLOW_MULTI_TOPIC")
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for merge in default_branch_merges(input, shell) {
            let dir = &merge.dir;
            for t in &merge.targets {
                if t == "--stdin" {
                    return vec![Finding::Deny(
                        "git merge --stdin はマージ対象を標準入力で受け取るので、新しい ADR の数を確かめられません。ブランチ名を引数で渡してください".to_string(),
                    )];
                }
                let Some(full) = merge_source(dir, t) else {
                    continue;
                };
                let Some(adrs) = added_adrs(dir, &format!("HEAD...{full}")) else {
                    return vec![Finding::Deny(format!(
                        "{t} が加える ADR を確認できません（git diff HEAD...{full} が失敗）"
                    ))];
                };
                if let Some(reason) = multi_topic_reason(&adrs, read_at(dir, &full)) {
                    return vec![Finding::Deny(reason)];
                }
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(src: &str) -> Option<Vec<String>> {
        let cs = Shell::parse(src).commands().to_vec();
        let (_, args) = cs[0].git_subcommand().unwrap();
        merge_targets(args)
    }

    #[test]
    fn reason_only_for_two_or_more() {
        let unreadable = |_: &str| None;
        assert_eq!(multi_topic_reason(&[], unreadable), None);
        assert_eq!(
            multi_topic_reason(&["docs/adr/0002-a.md".into()], unreadable),
            None
        );
        let r = multi_topic_reason(
            &["docs/adr/0002-a.md".into(), "docs/adr/0003-b.md".into()],
            unreadable,
        )
        .unwrap();
        assert!(r.contains("docs/adr/0002-a.md") && r.contains("docs/adr/0003-b.md"));
        assert!(r.contains("/topic-triage"));
        assert!(r.contains("ALLOW_MULTI_TOPIC=<理由>"));
        // 「〜の場合は別」のような条件を列挙しない（ADR-0023）
        assert!(!r.contains("まとめて加える"));
    }

    #[test]
    fn merge_targets_skip_option_values() {
        assert_eq!(
            targets("git merge --no-ff feat/a -m 'Merge: a'"),
            Some(vec!["feat/a".to_string()])
        );
        assert_eq!(
            targets("git merge -m x -s ort -X theirs --into-name main feat/a feat/b"),
            Some(vec!["feat/a".to_string(), "feat/b".to_string()])
        );
        assert_eq!(
            targets("git merge -F msg.txt --log=3 feat/a"),
            Some(vec!["feat/a".to_string()])
        );
        assert_eq!(targets("git merge -"), Some(vec!["-".to_string()]));
    }

    #[test]
    fn stdin_targets_are_unknown() {
        assert_eq!(
            targets("git merge --stdin"),
            Some(vec!["--stdin".to_string()])
        );
    }

    #[test]
    fn merge_control_is_not_a_target() {
        assert_eq!(targets("git merge --abort"), None);
        assert_eq!(targets("git merge --continue"), None);
        assert_eq!(targets("git merge --quit"), None);
    }

    fn linked(files: &[(&str, &str)], new: &[&str]) -> bool {
        let read = |p: &str| {
            files
                .iter()
                .find(|(n, _)| *n == p)
                .map(|(_, t)| t.to_string())
        };
        let new: Vec<String> = new.iter().map(|s| s.to_string()).collect();
        multi_topic_reason(&new, read).is_none()
    }

    const ADR_2: &str = "docs/adr/0002-a.md";
    const ADR_3: &str = "docs/adr/0003-b.md";
    const ADR_4: &str = "docs/adr/0004-c.md";

    #[test]
    fn adrs_linked_by_requires_are_one_topic() {
        let two = "---\nrequires: []\n---\n";
        let three = "---\nrequires: [ADR-0002]\n---\n";
        assert!(linked(&[(ADR_2, two), (ADR_3, three)], &[ADR_2, ADR_3]));
        // 向きは問わない
        let two = "---\nrequires: [ADR-0003]\n---\n";
        let three = "---\nrequires: []\n---\n";
        assert!(linked(&[(ADR_2, two), (ADR_3, three)], &[ADR_2, ADR_3]));
    }

    #[test]
    fn requires_chain_links_three_adrs_but_a_loose_one_does_not() {
        let none = "---\nrequires: []\n---\n";
        let on2 = "---\nrequires: [ADR-0002]\n---\n";
        let on3 = "---\nrequires: [ADR-0003]\n---\n";
        assert!(linked(
            &[(ADR_2, none), (ADR_3, on2), (ADR_4, on3)],
            &[ADR_2, ADR_3, ADR_4]
        ));
        assert!(!linked(
            &[(ADR_2, none), (ADR_3, on2), (ADR_4, none)],
            &[ADR_2, ADR_3, ADR_4]
        ));
    }

    #[test]
    fn malformed_requires_items_do_not_link() {
        for bad in ["ADR-2", "ADR-00002", "ADR-+002", "ADR-０００２"] {
            let three = format!("---\nrequires: [{bad}]\n---\n");
            assert!(
                !linked(
                    &[(ADR_2, "---\nrequires: []\n---\n"), (ADR_3, &three)],
                    &[ADR_2, ADR_3]
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn requires_outside_the_branch_or_unreadable_adrs_do_not_link() {
        let on1 = "---\nrequires: [ADR-0001]\n---\n";
        assert!(!linked(&[(ADR_2, on1), (ADR_3, on1)], &[ADR_2, ADR_3]));
        assert!(!linked(&[], &[ADR_2, ADR_3]));
        // フロントマターが読めない ADR はつなげない
        assert!(!linked(
            &[(ADR_2, "no front matter"), (ADR_3, "requires: [ADR-0002]")],
            &[ADR_2, ADR_3]
        ));
    }

    /// 手元の設定（グローバルの core.hooksPath など）を読まずに git を実行する。読むと、Claude Code のセッションの中では
    /// グローバルの commit-msg の hook がテストのコミットの件名を止める
    fn git_in(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", dir.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn moved_adr_is_not_added() {
        let dir =
            std::env::temp_dir().join(format!("claude-hooks-added-adrs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/decisions")).unwrap();
        git_in(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("docs/decisions/0001-x.md"), "x\n").unwrap();
        git_in(&dir, &["add", "."]);
        git_in(&dir, &["commit", "-q", "-m", "seed"]);
        std::fs::create_dir_all(dir.join("docs/adr")).unwrap();
        git_in(
            &dir,
            &["mv", "docs/decisions/0001-x.md", "docs/adr/0001-x.md"],
        );
        std::fs::write(dir.join("docs/adr/0002-y.md"), "y\n").unwrap();
        git_in(&dir, &["add", "."]);
        git_in(&dir, &["commit", "-q", "-m", "move"]);
        let added = added_adrs(&dir, "HEAD~1...HEAD");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(added, Some(vec!["docs/adr/0002-y.md".to_string()]));
    }
}
