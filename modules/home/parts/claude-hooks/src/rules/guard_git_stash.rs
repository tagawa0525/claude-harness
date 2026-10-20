//! guard-git-stash: `git stash`（`list` と `show` を除く）を止める
//!
//! stash はリポジトリの全 worktree と全セッションで共有なので、並行して作業するほかのセッションの退避と
//! 混ざり、pop や drop が他の作業の退避を取り込む・消す。退避は WIP のコミットか、対象の部分だけの
//! ステージで行う。読むだけの `list` / `show` は通す。サブコマンドの直後が `list` / `show` でなければ
//! （`git stash -u`、`git stash push`、サブコマンド省略の `git stash` を含め）すべて止める。エスケープはない。
//! 走らせるプログラムを決められない形も、文字列に `stash` があれば止める。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct GuardGitStash;

const DENY: &str = "git stash は使えません。stash はリポジトリの全 worktree と全セッションで共有なので、ほかの作業の退避と混ざります。\n退避したいときは WIP をコミットするか、テストの部分だけなど必要なファイルだけをステージしてください（git add <file>）。stash の中身を読むだけなら git stash list / git stash show は使えます。";

impl Rule for GuardGitStash {
    fn name(&self) -> &'static str {
        "guard-git-stash"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        let writes = shell.commands().iter().any(|cmd| {
            matches!(cmd.git_subcommand(), Some(("stash", args))
                if !matches!(args.first().map(|a| a.text.as_str()), Some("list" | "show")))
        });
        // 走らせるプログラムを決められない形（`$CMD`、`source`）は、`stash` が文字列にあれば止める側に倒す（ADR-0025）
        if writes || shell.runs_program("stash", |_| false) {
            vec![Finding::Deny(DENY.to_string())]
        } else {
            Vec::new()
        }
    }
}
