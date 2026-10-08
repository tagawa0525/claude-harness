//! guard-branch-base: 既定ブランチ以外の上で、起点を省いたブランチ作成を止める
//!
//! 起点を省くと新しいブランチは今の HEAD から作られ、作業中のブランチの未完了のコミット
//! （別のトピック）が入る。既定ブランチの上なら HEAD = 既定ブランチなので止めない。
//! 今のブランチの続きにするのが意図なら、起点を明示すれば（HEAD でも）通る。
//!
//! 対象: `git switch -c|-C <b>`、`git checkout -b|-B <b>`、`git branch <b>`、
//! `git worktree add <path> -b|-B <b>`、`git worktree add <path>`（パスの名前のブランチが
//! 無ければ HEAD から作られる）。いずれも起点（start-point / commit-ish）が無いもの。
//! 既定ブランチが見つからないリポジトリでは起点を示せないので何もしない。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct GuardBranchBase;

/// 起点を省いたブランチ作成
#[derive(Debug, PartialEq)]
struct NewBranch {
    name: String,
    /// `git worktree add <path>` の暗黙の作成。同名のブランチが既にあればチェックアウトするだけ
    implicit: bool,
    /// 起点に HEAD を明示した同じコマンド（今のブランチの続きにするときの例）
    with_head: String,
}

/// `git <sub> args…` が起点を省いたブランチ作成なら、その内容
fn new_branch(_sub: &str, _args: &[Arg]) -> Option<NewBranch> {
    todo!()
}

impl Rule for GuardBranchBase {
    fn name(&self) -> &'static str {
        "guard-branch-base"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            if let Some((sub, args)) = cmd.git_subcommand() {
                let _ = new_branch(sub, args);
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Option<NewBranch> {
        let cs = Shell::parse(src).commands();
        let (sub, args) = cs[0].git_subcommand()?;
        new_branch(sub, args)
    }

    fn name(src: &str) -> Option<String> {
        parse(src).map(|b| b.name)
    }

    #[test]
    fn switch_create_without_start() {
        let b = parse("git switch -c feat/a").unwrap();
        assert_eq!(b.name, "feat/a");
        assert!(!b.implicit);
        assert_eq!(b.with_head, "git switch -c feat/a HEAD");
        assert_eq!(name("git switch -q -C feat/a").as_deref(), Some("feat/a"));
        assert_eq!(
            name("git switch --create=feat/a").as_deref(),
            Some("feat/a")
        );
        assert_eq!(
            name("git switch --force-create feat/a").as_deref(),
            Some("feat/a")
        );
    }

    #[test]
    fn switch_with_start_or_without_create_is_not_a_target() {
        assert_eq!(parse("git switch -c feat/a main"), None);
        assert_eq!(parse("git switch -c feat/a --no-track origin/main"), None);
        assert_eq!(parse("git switch main"), None);
        assert_eq!(parse("git switch -"), None);
        assert_eq!(parse("git switch --orphan fresh"), None);
    }

    #[test]
    fn checkout_new_branch() {
        let b = parse("git checkout -b feat/a").unwrap();
        assert_eq!(b.with_head, "git checkout -b feat/a HEAD");
        assert_eq!(name("git checkout -B feat/a").as_deref(), Some("feat/a"));
        assert_eq!(parse("git checkout -b feat/a origin/main"), None);
        assert_eq!(parse("git checkout main"), None);
        assert_eq!(parse("git checkout -- a.txt"), None);
        assert_eq!(parse("git checkout --orphan fresh"), None);
    }

    #[test]
    fn branch_create() {
        let b = parse("git branch feat/a").unwrap();
        assert_eq!(b.with_head, "git branch feat/a HEAD");
        assert_eq!(
            name("git branch --no-track feat/a").as_deref(),
            Some("feat/a")
        );
        assert_eq!(name("git branch -f feat/a").as_deref(), Some("feat/a"));
        assert_eq!(parse("git branch feat/a main"), None);
        assert_eq!(parse("git branch -t feat/a origin/feat/a"), None);
    }

    #[test]
    fn branch_other_modes_are_not_a_target() {
        for src in [
            "git branch",
            "git branch -a",
            "git branch -vv",
            "git branch -d feat/a",
            "git branch -D feat/a",
            "git branch --delete feat/a",
            "git branch -m feat/b",
            "git branch -M feat/a feat/b",
            "git branch -c feat/b",
            "git branch --show-current",
            "git branch --list 'feat/*'",
            "git branch --contains HEAD",
            "git branch --merged main",
            "git branch -u origin/main",
            "git branch --set-upstream-to=origin/main",
            "git branch --unset-upstream",
            "git branch --edit-description",
        ] {
            assert_eq!(parse(src), None, "{src}");
        }
    }

    #[test]
    fn worktree_add() {
        let b = parse("git worktree add ../wt -b feat/a").unwrap();
        assert_eq!(b.name, "feat/a");
        assert!(!b.implicit);
        assert_eq!(b.with_head, "git worktree add ../wt -b feat/a HEAD");
        assert_eq!(
            name("git worktree add -B feat/a ../wt").as_deref(),
            Some("feat/a")
        );
        assert_eq!(
            name("git worktree add --lock --reason 'x y' -b feat/a ../wt").as_deref(),
            Some("feat/a")
        );
        assert_eq!(parse("git worktree add ../wt -b feat/a main"), None);
        assert_eq!(parse("git worktree add -b feat/a ../wt HEAD"), None);
        assert_eq!(parse("git worktree add ../wt feat/a"), None);
        assert_eq!(parse("git worktree add --detach ../wt"), None);
        assert_eq!(parse("git worktree add --orphan -b fresh ../wt"), None);
        assert_eq!(parse("git worktree list"), None);
        assert_eq!(parse("git worktree remove ../wt"), None);
    }

    #[test]
    fn worktree_add_path_only_creates_branch_named_after_path() {
        let b = parse("git worktree add ../x/newname").unwrap();
        assert_eq!(b.name, "newname");
        assert!(b.implicit);
        assert_eq!(b.with_head, "git worktree add -b newname ../x/newname HEAD");
    }
}
