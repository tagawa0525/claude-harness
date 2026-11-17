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
//!
//! 既定ブランチでない HEAD で、そのブランチ自身をマージするコマンドも止める。既定ブランチの worktree でなく対象の
//! ブランチの worktree で実行すると、git は「Already up to date」で何もせずに 0 で終わり、マージしたと読み違える。
//! ほかのブランチを取り込むマージ（main を feature ブランチへ、など）は普通の操作なので止めない

use super::Rule;
use super::guard_branch_base::branch_at;
use super::pre_git_merge_check::{
    commit_ish, default_branch_merges, local_branch_ref, merge_source, merge_targets,
};
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

/// 既定ブランチでない HEAD で、そのブランチ自身をマージする `git merge` ごとの deny
fn self_merges(input: &Input, shell: &Shell) -> Vec<Finding> {
    let mut out = Vec::new();
    for cmd in shell.commands() {
        let Some(targets) = cmd
            .git_subcommand()
            .filter(|(sub, _)| *sub == "merge")
            .and_then(|(_, args)| merge_targets(args))
        else {
            continue;
        };
        let dir = shell.target_dir(cmd, &input.cwd);
        let Some(default_ref) = git::default_branch(&dir) else {
            continue;
        };
        let default = git::local_name(&default_ref);
        // detached HEAD（空）はブランチ自身へのマージにならない
        let Some(head) = branch_at(shell, cmd, &input.cwd, &dir)
            .filter(|b| !b.is_empty() && b.as_str() != default)
        else {
            continue;
        };
        let head_ref = format!("refs/heads/{head}");
        // `-` と `@{-N}`（前に居たブランチ）は、同じコマンドの前の switch で変わる。hook の時点で解くと、switch の
        // 前の履歴を指して誤るので見ない（それが今のブランチ自身を指すことは、普通の操作では無い）
        let previous = |t: &str| t == "-" || t.starts_with("@{-");
        if targets
            .iter()
            .filter(|t| !previous(t))
            .any(|t| local_branch_ref(&dir, t).as_deref() == Some(head_ref.as_str()))
        {
            out.push(Finding::Deny(format!(
                "{head} を {head} 自身にマージしようとしています（{} の HEAD は {default} でなく {head}）。git は何もせずに「Already up to date」で終わり、{default} には入りません。{default} の worktree で実行する（git -C <{default} の worktree> merge …）か、{default} の worktree が無ければ git switch {default} してからマージしてください",
                dir.display()
            )));
        }
    }
    out
}

pub struct GuardGitMerge;

impl Rule for GuardGitMerge {
    fn name(&self) -> &'static str {
        "guard-git-merge"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut out = self_merges(input, shell);
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
