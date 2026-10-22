//! guard-git-merge: 既定ブランチへのローカルの git merge は、`--no-ff` で、既定ブランチの上に
//! rebase 済みのブランチだけを通す
//!
//! GitHub を使わないリポジトリは feature branch を main に `git merge --no-ff` する（/git-merge）。
//! gh pr merge の pre-merge-check が `--merge`（マージコミットを作る）と「head が base より
//! 遅れていない」を強制するのと同じ形を、ローカルのマージにも課す。fast-forward や `--squash` では
//! ブランチの履歴とマージの単位が残らず、遅れたまま merge すると git graph が分かれる。
//! 対象のブランチをチェックアウトしている worktree に未コミットの変更か未追跡のファイル（無視された
//! ファイルは含まない）があれば止める。残ったままマージすると、後片付けの `git worktree remove` が
//! 止まる。/git-merge は git-merge-state.sh で同じ条件を見るが、手で git merge すると通らない。
//! 既定ブランチの worktree は見ない（git が自分で、未コミットの変更とぶつかるときに止める）。
//! 対象は pre-git-merge-check と同じ（ローカルのブランチを既定ブランチの上でマージするときだけ。
//! リモート追跡ブランチの取り込み、中断・再開は対象外）。エスケープはない（修正方法は理由に示す）

use super::Rule;
use super::pre_git_merge_check::{default_branch_merges, local_branch_ref};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::{Path, PathBuf};

/// deny の理由に示す、未コミットの変更の先頭の件数
const SHOWN_CHANGES: usize = 5;

/// ブランチ（refs/heads/…）をチェックアウトしている worktree。チェックアウトされていなければ None
fn worktree_of(dir: &Path, full_ref: &str) -> Option<PathBuf> {
    let list = git::git(dir, &["worktree", "list", "--porcelain"])?;
    let mut path = None;
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if line.strip_prefix("branch ") == Some(full_ref) {
            return path;
        }
    }
    None
}

/// worktree の未コミットの変更と未追跡のファイルを理由にした deny（無ければ None）
fn dirty_reason(branch: &str, worktree: &Path) -> Option<String> {
    let status = git::git(worktree, &["status", "--porcelain"]).filter(|s| !s.is_empty())?;
    let lines: Vec<&str> = status.lines().collect();
    let shown: Vec<String> = lines
        .iter()
        .take(SHOWN_CHANGES)
        .map(|l| format!("  {l}"))
        .collect();
    let more = match lines.len().saturating_sub(SHOWN_CHANGES) {
        0 => String::new(),
        n => format!("\n  …ほか {n} 件"),
    };
    Some(format!(
        "{branch} の worktree {} に未コミットの変更か未追跡のファイルがあります（{} 件）:\n{}{more}\n\
         残したままマージすると、後片付けの git worktree remove が止まります。コミットする、リポジトリの規則（.gitignore）で無視する、要らなければ消すのいずれかで worktree を空にしてから、マージしてください",
        worktree.display(),
        lines.len(),
        shown.join("\n")
    ))
}

pub struct GuardGitMerge;

impl Rule for GuardGitMerge {
    fn name(&self) -> &'static str {
        "guard-git-merge"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut out = Vec::new();
        for merge in default_branch_merges(input, shell) {
            let branches: Vec<(&str, String)> = merge
                .targets
                .iter()
                .filter_map(|t| local_branch_ref(&merge.dir, t).map(|full| (t.as_str(), full)))
                .collect();
            if branches.is_empty() {
                continue;
            }
            let default = &merge.default;
            if !merge.no_ff {
                out.push(Finding::Deny(format!(
                    "{default} へのマージは --no-ff を付けてマージコミットを作ってください（--ff / --ff-only / --squash は併用できません。fast-forward や squash では、ブランチの履歴とマージの単位が残りません）"
                )));
            }
            for (name, full) in branches {
                let up_to_date =
                    git::git(&merge.dir, &["merge-base", "--is-ancestor", default, &full])
                        .is_some();
                let branch = full.strip_prefix("refs/heads/").unwrap_or(name);
                if let Some(reason) =
                    worktree_of(&merge.dir, &full).and_then(|wt| dirty_reason(branch, &wt))
                {
                    out.push(Finding::Deny(reason));
                }
                if !up_to_date {
                    out.push(Finding::Deny(format!(
                        "{branch} は {default} より遅れています（{default} にあるコミットが入っていません）。git switch {branch} && git rebase {default} で {default} の上に載せてから、チェックをやり直してマージしてください"
                    )));
                }
            }
        }
        out
    }
}
