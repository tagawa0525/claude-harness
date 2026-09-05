//! block-main-commit: main / master ブランチでの git commit を止める
//!
//! CLAUDE.md の「main ブランチに直接コミットしない」をツールレベルで強制する。
//! 例外は git 側の pre-commit hook（modules/home/parts/git.nix）と必ず揃える
//! （modules/home/parts/tests/main-commit-gates.sh が一致を検証する）:
//! - GitHub リモートがなければ PR フロー適用外（PR を作れない以上 feature branch を強制しても
//!   マージする手段がない）
//! - flake.lock だけの変更（M 1 件）は nix-rebuild update の正規フローなので通す

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockMainCommit;

impl Rule for BlockMainCommit {
    fn name(&self) -> &'static str {
        "block-main-commit"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            if cmd.git_subcommand().map(|(sub, _)| sub) != Some("commit") {
                continue;
            }
            let dir = shell.target_dir(&cmd, &input.cwd);
            if !git::has_github_remote(&dir) {
                continue;
            }
            let branch = git::current_branch(&dir);
            if branch != "main" && branch != "master" {
                continue;
            }
            let staged = git::git(&dir, &["diff", "--cached", "--name-status"]).unwrap_or_default();
            if staged == "M\tflake.lock" {
                continue;
            }
            return vec![Finding::Deny(
                "mainブランチへの直接コミットは禁止されています。featureブランチを作成してください: /git-branch".to_string(),
            )];
        }
        Vec::new()
    }
}
