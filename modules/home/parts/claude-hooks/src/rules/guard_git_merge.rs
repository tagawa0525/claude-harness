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
use super::pre_git_merge_check::{commit_ish, default_branch_merges, merge_source};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::{Path, PathBuf};

/// deny の理由に示す、未コミットの変更の先頭の件数
const SHOWN_CHANGES: usize = 5;

/// マージ元をチェックアウトしている worktree と、そのブランチ名。ブランチの ref（refs/heads/…）はそのブランチの
/// worktree、ブランチ名でないコミットの指定（SHA、`feat/x^0`）は、HEAD がそのコミットで、ブランチをチェックアウト
/// している worktree（detached は後片付けの対象でない）。全部を返す（無ければ空）
fn worktrees_of(dir: &Path, source: &str) -> Vec<(PathBuf, String)> {
    let Some(list) = git::git(dir, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    // 注釈つきタグのオブジェクトはコミットに剥がして、worktree の HEAD と比べる
    let Some(commit) = commit_ish(dir, source)
        .and_then(|id| git::git(dir, &["rev-parse", "--verify", &format!("{id}^{{commit}}")]))
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let by_ref = source.starts_with("refs/heads/");
    for block in list.split("\n\n") {
        let (mut path, mut head, mut branch) = (None, None, None);
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                head = Some(h);
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b);
            }
        }
        let (Some(path), Some(head), Some(branch)) = (path, head, branch) else {
            continue;
        };
        let hit = if by_ref {
            branch == source
        } else {
            head == commit
        };
        if hit {
            let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
            found.push((path, name.to_string()));
        }
    }
    found
}

/// worktree の未コミットの変更と未追跡のファイルを理由にした deny（無ければ None）。
/// 他のエージェントが作業中の worktree の index を更新して `index.lock` を取らないよう、
/// `--no-optional-locks` で読む
fn dirty_reason(branch: &str, worktree: &Path) -> Option<String> {
    let status = git::git(worktree, &["--no-optional-locks", "status", "--porcelain"])
        .filter(|s| !s.is_empty())?;
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
                .filter_map(|t| merge_source(&merge.dir, t).map(|full| (t.as_str(), full)))
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
                for (wt, wt_branch) in worktrees_of(&merge.dir, &full) {
                    if let Some(reason) = dirty_reason(&wt_branch, &wt) {
                        out.push(Finding::Deny(reason));
                    }
                }
                if !up_to_date {
                    out.push(Finding::Deny(format!(
                        "{branch} は {default} より遅れています（{default} にあるコミットが入っていません）。{branch} を {default} の上に rebase（ブランチなら git switch {branch} && git rebase {default}）してから、チェックをやり直してマージしてください"
                    )));
                }
            }
        }
        out
    }
}
