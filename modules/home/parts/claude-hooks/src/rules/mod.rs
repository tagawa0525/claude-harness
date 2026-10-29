//! ルール = 1 つのツールの呼び出しへの 1 つの検査。Bash のルールの名前は、移す前の bash hook のファイル名から .sh を除いたもの

use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Escape, Shell};

pub trait Rule: Sync {
    fn name(&self) -> &'static str;
    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding>;
    /// 検査する Claude Code のツールの名前（hook の入力の tool_name）
    fn tool(&self) -> &'static str {
        "Bash"
    }
    /// ルール全体を外すエスケープの名前。`check` の結果に `apply_escape` で当てる（ADR-0023）。
    /// 検査ごとに別のエスケープを持つルールは None を返し、`check` の中で当てる
    fn escape(&self) -> Option<&'static str> {
        None
    }
}

/// ルールを評価し、エスケープを当てた結果を返す。理由つきなら評価しない（gitleaks や gh の呼び出しを
/// 伴うルールを、外す指定があるのに実行しない）。値が短ければ評価し、止めるときだけ理由を求める
pub fn run(rule: &dyn Rule, input: &Input, shell: &Shell) -> Vec<Finding> {
    let Some(name) = rule.escape() else {
        return rule.check(input, shell);
    };
    match shell.escape(name) {
        Escape::Given => Vec::new(),
        Escape::Absent => rule.check(input, shell),
        esc @ (Escape::Short | Escape::Expanded) => {
            apply_escape(esc, name, rule.check(input, shell))
        }
    }
}

/// 止める判定にエスケープを当てる（ADR-0023）。止めないならエスケープは見ない（付けていても咎めない）。
/// 理由つきなら通す。値が理由として短ければ、本来の文言（何を止めたか、正規の修正方法）に理由を求める行を足す
pub fn apply_escape(esc: Escape, name: &str, mut findings: Vec<Finding>) -> Vec<Finding> {
    if findings.is_empty() {
        return findings;
    }
    match esc {
        Escape::Absent => findings,
        Escape::Short | Escape::Expanded => {
            findings.push(Finding::Deny(reason_required(name)));
            findings
        }
        Escape::Given => Vec::new(),
    }
}

/// 理由の文字列の列（pre-merge-check の reasons）にエスケープを当てる
pub fn apply_escape_to_reasons(esc: Escape, name: &str, mut reasons: Vec<String>) -> Vec<String> {
    if reasons.is_empty() {
        return reasons;
    }
    match esc {
        Escape::Absent => reasons,
        Escape::Short | Escape::Expanded => {
            reasons.push(reason_required(name));
            reasons
        }
        Escape::Given => Vec::new(),
    }
}

/// 値が短いエスケープを止める文言。しきい値は書かない（長さに合わせた無意味な値を誘うため）
pub fn reason_required(name: &str) -> String {
    format!("{name} には理由を記載してください（{name}=<理由> の形で、値はそのまま記録に残ります）")
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
mod check_merge_body_ja_terms;
mod delegate_request_fields;
mod guard_branch_base;
mod guard_exclusive_slurm;
mod guard_gh_api;
mod guard_gh_run_rerun;
mod guard_git_add;
mod guard_git_amend;
mod guard_git_merge;
mod guard_git_push;
mod guard_git_reset_hard;
mod guard_git_stash;
mod guard_sbatch;
mod pre_git_merge_check;
mod pre_merge_check;
pub use pre_merge_check::local_review_covers_for_pr;
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
        &check_merge_body_ja_terms::CheckMergeBodyJaTerms,
        &guard_git_merge::GuardGitMerge,
        &guard_git_stash::GuardGitStash,
        &guard_git_reset_hard::GuardGitResetHard,
        &guard_git_amend::GuardGitAmend,
        &guard_exclusive_slurm::GuardExclusiveSlurm,
        &delegate_request_fields::DelegateRequestFields,
    ]
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|r| r.name()).collect()
}

pub fn by_name(name: &str) -> Option<&'static dyn Rule> {
    all().iter().copied().find(|r| r.name() == name)
}

/// tool_name のツールを検査するルール
pub fn for_tool(tool_name: &str) -> Vec<&'static dyn Rule> {
    all()
        .iter()
        .copied()
        .filter(|r| r.tool() == tool_name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_for(tool_name: &str) -> Vec<&'static str> {
        for_tool(tool_name).iter().map(|r| r.name()).collect()
    }

    #[test]
    fn agent_tool_runs_only_the_delegate_request_check() {
        assert_eq!(names_for("Agent"), ["delegate-request-fields"]);
    }

    #[test]
    fn bash_tool_does_not_run_the_delegate_request_check() {
        let bash = names_for("Bash");
        assert!(bash.contains(&"guard-git-stash"));
        assert!(bash.contains(&"guard-git-reset-hard"));
        assert!(!bash.contains(&"delegate-request-fields"));
    }

    #[test]
    fn other_tools_run_no_rule() {
        assert!(names_for("Read").is_empty());
    }
}
