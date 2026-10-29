//! guard-git-amend: 先端が既定ブランチに含まれる feature ブランチでの `git commit --amend` を止める
//!
//! 既定ブランチから作ったばかりのブランチの先端は、既定ブランチのコミット（多くはマージコミット）である。
//! 最初の `git commit` が hook（commit-msg など）に拒まれたのに気づかずに amend すると、そのコミットを
//! 書き換えたもの（マージコミットなら親 2 つ）がブランチの最初のコミットになる。amend は新しいコミットを
//! 作るので既定ブランチは変わらず、post-rewrite の hook は書き換えた番号を文書に付け替える。
//!
//! 止めるのは、今のブランチが名前のあるブランチで既定ブランチでなく、その先端が既定ブランチ
//! （[`git::default_branch`]）に含まれる（`git merge-base --is-ancestor`）ときだけ。既定ブランチの上の amend
//! （マージの直後にマージコミットの本文を直す）、detached HEAD（rebase の途中の edit）、ブランチに固有の
//! コミットがあるときは通す。同じコマンドの前の `git switch` / `checkout` は、その先のブランチで判定する
//! （[`branch_at`]。まだ無いブランチを作って移るなら、起点の今の HEAD を見る）。ほかの前のコマンド
//! （`git commit` を含む）は見ず、実行前の HEAD で判定する。前の commit が拒まれれば、amend は実行前の
//! HEAD を書き換えるため。エスケープは無い。既定ブランチのコミットを feature ブランチで書き換えて残す
//! 正当な手順が無いため（直すのは既定ブランチの上の amend か、新しい commit）。比べた案は ADR-0069。

use super::Rule;
use super::guard_branch_base::branch_at;
use crate::git;
use crate::git_args::{Item, items};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};

pub struct GuardGitAmend;

/// `git commit` の引数が amend を指すか。git は一意な略（`--am` など）を受け付け、後に書いた
/// `--no-amend` が前の `--amend` を打ち消す。値（`-m --amend`）と `--` の後の pathspec は数えない
fn is_amend(cmd: &Cmd) -> bool {
    let Some(("commit", args)) = cmd.git_subcommand() else {
        return false;
    };
    let mut amend = false;
    for item in items("commit", args) {
        match item {
            Item::Long(name) if name.len() >= 4 && "--amend".starts_with(name) => amend = true,
            Item::Long("--no-amend") => amend = false,
            _ => {}
        }
    }
    amend
}

impl Rule for GuardGitAmend {
    fn name(&self) -> &'static str {
        "guard-git-amend"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            if !is_amend(cmd) || shell.runs_under_ssh_or_login(cmd) {
                continue;
            }
            let dir = shell.target_dir(cmd, &input.cwd);
            if git::git(&dir, &["rev-parse", "--git-dir"]).is_none() {
                continue;
            }
            let Some(default_ref) = git::default_branch(&dir) else {
                continue;
            };
            let default = git::local_name(&default_ref);
            // 移った先が分からない前の git switch（`-`、`--detach`）は、どのコミットを書き換えるかが分からない。
            // 書き換える先が既定ブランチのコミットとは確かめられないので通す
            let Some(branch) = branch_at(shell, cmd, &input.cwd, &dir) else {
                continue;
            };
            if branch.is_empty() || branch == default {
                continue;
            }
            let local = format!("refs/heads/{branch}");
            let tip = git::git(&dir, &["rev-parse", "--verify", "--quiet", &local])
                .unwrap_or_else(|| "HEAD".to_string());
            if git::git(&dir, &["merge-base", "--is-ancestor", &tip, &default_ref]).is_none() {
                continue;
            }
            return vec![Finding::Deny(format!(
                "ブランチ {branch} の先端は既定ブランチ（{default_ref}）に含まれるコミットです。ここで amend すると、\
                 既定ブランチのコミット（多くはマージコミット）を書き換えたものがブランチの最初のコミットになります\
                 （マージコミットなら親 2 つ。post-rewrite の hook が文書の番号も付け替えます）。\n\
                 - 直前の git commit が hook に拒まれていないかを git log -1 で確かめる（-q や ; のつなぎでは失敗が見えません）\n\
                 - ブランチの最初のコミットを作るなら、amend せず git commit で新しいコミットを作る\n\
                 - 既定ブランチのコミット（マージコミットの本文など）を直すのが意図なら、{default} の上で amend する"
            ))];
        }
        Vec::new()
    }
}
