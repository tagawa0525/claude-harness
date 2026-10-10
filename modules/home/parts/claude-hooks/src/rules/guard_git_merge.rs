//! guard-git-merge: 既定ブランチへのローカルの git merge は、`--no-ff` で、既定ブランチの上に
//! rebase 済みのブランチだけを通す
//!
//! GitHub を使わないリポジトリは feature branch を main に `git merge --no-ff` する（/git-merge）。
//! gh pr merge の pre-merge-check が `--merge`（マージコミットを作る）と「head が base より
//! 遅れていない」を強制するのと同じ形を、ローカルのマージにも課す。fast-forward や `--squash` では
//! ブランチの履歴とマージの単位が残らず、遅れたまま merge すると git graph が分かれる。
//! 対象は pre-git-merge-check と同じ（ローカルのブランチを既定ブランチの上でマージするときだけ。
//! リモート追跡ブランチの取り込み、中断・再開は対象外）。エスケープはない（直し方は理由に示す）

use super::Rule;
use super::pre_git_merge_check::{default_branch_merges, local_branch_ref};
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct GuardGitMerge;

impl Rule for GuardGitMerge {
    fn name(&self) -> &'static str {
        "guard-git-merge"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut out = Vec::new();
        for merge in default_branch_merges(input, shell) {
            let branches: Vec<(&str, String)> = merge
                .targets
                .iter()
                .filter_map(|t| local_branch_ref(&merge.dir, t).map(|full| (t.as_str(), full)))
                .collect();
            if branches.is_empty() {
                continue;
            }
            let default = &merge.default;
            if !merge.no_ff {
                out.push(Finding::Deny(format!(
                    "{default} へのマージは --no-ff を付けてマージコミットを作ってください（fast-forward や --squash では、ブランチの履歴とマージの単位が残りません）"
                )));
            }
            for (name, full) in branches {
                let up_to_date =
                    git::git(&merge.dir, &["merge-base", "--is-ancestor", default, &full])
                        .is_some();
                if !up_to_date {
                    let branch = full.strip_prefix("refs/heads/").unwrap_or(name);
                    out.push(Finding::Deny(format!(
                        "{branch} は {default} より遅れています（{default} にあるコミットが入っていません）。git switch {branch} && git rebase {default} で {default} の上に載せてから、チェックをやり直してマージしてください"
                    )));
                }
            }
        }
        out
    }
}
