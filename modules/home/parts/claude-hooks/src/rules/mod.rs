//! ルール = 旧 bash hook 1 本。名前は旧ファイル名から .sh を除いたもの

use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub trait Rule: Sync {
    fn name(&self) -> &'static str;
    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding>;
}

mod block_main_commit;
mod block_secret_commit;
mod guard_gh_api;
mod guard_gh_run_rerun;
mod guard_git_add;
mod guard_git_push;
mod pre_merge_check;
mod pre_pr_create_check;
mod require_background_wait;
mod warn_large_commit;

pub fn all() -> &'static [&'static dyn Rule] {
    &[
        &require_background_wait::RequireBackgroundWait,
        &block_main_commit::BlockMainCommit,
        &warn_large_commit::WarnLargeCommit,
        &guard_git_add::GuardGitAdd,
        &guard_git_push::GuardGitPush,
        &block_secret_commit::BlockSecretCommit,
        &guard_gh_run_rerun::GuardGhRunRerun,
        &guard_gh_api::GuardGhApi,
        &pre_pr_create_check::PrePrCreateCheck,
        &pre_merge_check::PreMergeCheck,
    ]
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|r| r.name()).collect()
}

pub fn by_name(name: &str) -> Option<&'static dyn Rule> {
    all().iter().copied().find(|r| r.name() == name)
}
