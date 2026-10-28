//! guard-git-reset-hard: 未コミットの変更がある worktree での `git reset --hard` を止める
//!
//! `reset --hard` は追跡しているファイルの未コミットの変更を戻せない形で消す。worktree は並行するほかのセッションと
//! 共有しうるので（メインの worktree など）、変更がそのセッションのものとは限らない。検査するのは、コマンドの
//! 対象のディレクトリ（前の `cd` と `-C` を当てたもの）と、`-C` を書いていなければ hook の cwd も。前の `cd` が
//! 失敗すると、`;` の後の reset は hook の cwd で走るため（`cd <一時のリポジトリ> && …; git reset --hard` が、
//! 移れなかったときにメインの worktree を戻した）。`-C` で指せば `cd` の成否に依らないので、それを修正方法に示す。
//! 追跡していないファイルは reset --hard が消さないので数えない。比べた案は ADR-0062。

use super::{Rule, escape_hint};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Shell, has_flag};
use std::path::PathBuf;

pub struct GuardGitResetHard;

const ESCAPE: &str = "ALLOW_GIT_RESET_HARD";

impl Rule for GuardGitResetHard {
    fn name(&self) -> &'static str {
        "guard-git-reset-hard"
    }

    fn escape(&self) -> Option<&'static str> {
        Some(ESCAPE)
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut dirty: Vec<(PathBuf, String)> = Vec::new();
        for cmd in shell.commands() {
            let Some(("reset", args)) = cmd.git_subcommand() else {
                continue;
            };
            if !has_flag(args, &["--hard"]) {
                continue;
            }
            let mut dirs = vec![shell.target_dir(cmd, &input.cwd)];
            if cmd.git.as_ref().is_some_and(|g| g.dirs.is_empty()) {
                dirs.push(input.cwd.clone());
            }
            for dir in dirs {
                if git::not_a_directory(&dir) || dirty.iter().any(|(d, _)| *d == dir) {
                    continue;
                }
                if let Some(status) =
                    git::git(&dir, &["status", "--porcelain", "--untracked-files=no"])
                    && !status.is_empty()
                {
                    dirty.push((dir, status));
                }
            }
        }
        if dirty.is_empty() {
            return Vec::new();
        }
        let listing: String = dirty
            .iter()
            .map(|(dir, status)| {
                let files: String = status.lines().map(|l| format!("\n     {l}")).collect();
                format!("\n  {}:{files}", dir.display())
            })
            .collect();
        vec![Finding::Deny(format!(
            "git reset --hard は、次の worktree の未コミットの変更を戻せない形で消します（ほかのセッションの変更かもしれません）:{listing}\n\
             前の cd が失敗すると、; の後の reset は hook の cwd で走るので、cwd も検査しています。\n\
             対象は git -C <dir> reset --hard で指してください（cd の成否に依りません）。残す変更は WIP としてコミットしてください。\n{}",
            escape_hint(ESCAPE)
        ))]
    }
}
