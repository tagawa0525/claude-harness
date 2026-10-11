//! ルール = 旧 bash hook 1 本。名前は旧ファイル名から .sh を除いたもの

use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Escape, Shell};

pub trait Rule: Sync {
    fn name(&self) -> &'static str;
    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding>;
}

/// エスケープを読む（ADR-0023）。理由つきなら検査を飛ばす（空の findings）、値が理由として短ければ
/// 理由を求めて止める、無ければ None で検査に進む
pub fn escape(shell: &Shell, name: &str) -> Option<Vec<Finding>> {
    match shell.escape(name) {
        Escape::Absent => None,
        Escape::Short => Some(vec![reason_required(name)]),
        Escape::Given => Some(Vec::new()),
    }
}

/// 値が短いエスケープを止める文言。しきい値は書かない（長さに合わせた無意味な値を誘うため）
pub fn reason_required(name: &str) -> Finding {
    Finding::Deny(format!(
        "{name} には理由を記載してください（{name}=<理由> の形で、値はそのまま記録に残ります）"
    ))
}

/// deny 文言の末尾に置く、エスケープの付け方の案内。「〜なら付けてよい」の条件は列挙しない
pub fn escape_hint(name: &str) -> String {
    format!(
        "どうしても外すときは、理由を値にして {name}=<理由> をコマンドに付けてください（理由はそのまま記録に残ります）。その理由が機械で判定できるものなら hook の不足なので、issue に書いてください"
    )
}

mod block_main_commit;
mod block_no_verify;
mod block_secret_commit;
mod guard_branch_base;
mod guard_gh_api;
mod guard_gh_run_rerun;
mod guard_git_add;
mod guard_git_merge;
mod guard_git_push;
mod guard_sbatch;
mod pre_git_merge_check;
mod pre_merge_check;
mod pre_pr_create_check;
mod require_background_wait;
mod warn_large_commit;

pub fn all() -> &'static [&'static dyn Rule] {
    &[
        &require_background_wait::RequireBackgroundWait,
        &guard_sbatch::GuardSbatch,
        &block_main_commit::BlockMainCommit,
        &warn_large_commit::WarnLargeCommit,
        &guard_git_add::GuardGitAdd,
        &guard_git_push::GuardGitPush,
        &guard_branch_base::GuardBranchBase,
        &block_secret_commit::BlockSecretCommit,
        &block_no_verify::BlockNoVerify,
        &guard_gh_run_rerun::GuardGhRunRerun,
        &guard_gh_api::GuardGhApi,
        &pre_pr_create_check::PrePrCreateCheck,
        &pre_merge_check::PreMergeCheck,
        &pre_git_merge_check::PreGitMergeCheck,
        &guard_git_merge::GuardGitMerge,
    ]
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|r| r.name()).collect()
}

pub fn by_name(name: &str) -> Option<&'static dyn Rule> {
    all().iter().copied().find(|r| r.name() == name)
}
