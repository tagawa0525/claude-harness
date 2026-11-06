//! block-main-commit: main / master ブランチでの git commit を止める
//!
//! CLAUDE.md の「main ブランチに直接コミットしない」をツールレベルで強制する。リモートの有無によらず止める
//! （GitHub のリモートが無いリポジトリも、ブランチは /git-merge でローカルの main にマージする）。
//! 例外は git 側の pre-commit hook（modules/home/parts/git.nix）と必ず揃える
//! （modules/home/parts/tests/main-commit-gates.sh が一致を検証する）:
//! - flake.lock だけの変更（M 1 件）は nix-rebuild update の正規フローなので通す
//! - マージを締めるコミット（MERGE_HEAD がある）は、/git-merge で本文の語に拒まれた後や競合を解決した後に
//!   `git commit` でマージを終える形なので通す
//! - HEAD の無いリポジトリの最初のコミットは、分けるブランチの起点が無いので通す

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct BlockMainCommit;

/// 拒否の文言。git の pre-commit（modules/home/parts/git.nix）も同じ趣旨の文言を出す
const DENY_MESSAGE: &str = "main / master ブランチへの直接コミットは禁止されています。featureブランチを作成してください: /git-branch\n\
- GitHub のリモートが無いリポジトリでも、ブランチは /git-merge で main にマージするので、main に直接コミットする理由はありません\
（2026-11-06 に、cd が失敗した後の git commit が別のリポジトリの main に空のコミットを作りました）\n\
- 対象のディレクトリを確かめる（前の cd が失敗すると、git は hook の cwd のリポジトリで走ります）\n\
- 試すための一時のリポジトリなら、main / master 以外のブランチで作る（git init -b <名前>、git switch -c <名前>）\n\
- 通すのは、flake.lock だけの変更、マージを締めるコミット（MERGE_HEAD がある）、HEAD の無いリポジトリの最初のコミットです";

impl Rule for BlockMainCommit {
    fn name(&self) -> &'static str {
        "block-main-commit"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            if cmd.git_subcommand().map(|(sub, _)| sub) != Some("commit") {
                continue;
            }
            let dir = shell.target_dir(cmd, &input.cwd);
            let branch = git::current_branch(&dir);
            if branch != "main" && branch != "master" {
                continue;
            }
            if git::git(&dir, &["rev-parse", "-q", "--verify", "HEAD"]).is_none() {
                continue;
            }
            if git::git(&dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_some() {
                continue;
            }
            let staged = git::git(&dir, &["diff", "--cached", "--name-status"]).unwrap_or_default();
            if staged == "M\tflake.lock" {
                continue;
            }
            return vec![Finding::Deny(DENY_MESSAGE.to_string())];
        }
        Vec::new()
    }
}
