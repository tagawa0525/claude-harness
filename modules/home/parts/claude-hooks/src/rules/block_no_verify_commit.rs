//! block-no-verify-commit: git の hook を飛ばす `git commit --no-verify`（`-n`）を止める
//!
//! 機密情報の検査（gitleaks）、main への直接コミットのガード、形式の検査は git の pre-commit と commit-msg に
//! あり（ADR-0015）、--no-verify はそれを全て飛ばす。コマンドを実行する前の PreToolUse で止める。
//!
//! エスケープ: コマンドに `ALLOW_NO_VERIFY=1` を付ける。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Shell, short_cluster_has};

pub struct BlockNoVerifyCommit;

impl Rule for BlockNoVerifyCommit {
    fn name(&self) -> &'static str {
        "block-no-verify-commit"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_NO_VERIFY") {
            return Vec::new();
        }
        for cmd in shell.commands() {
            let Some(("commit", args)) = cmd.git_subcommand() else {
                continue;
            };
            if args
                .iter()
                .any(|a| a.text == "--no-verify" || short_cluster_has(a, 'n'))
            {
                return vec![Finding::Deny(
                    "git commit --no-verify（-n）は、git の hook の検査（機密情報、main への直接コミット、形式）を飛ばすので止めます（ADR-0015）。\nhook が止めた理由を直してからコミットしてください。どうしても必要な場合は ALLOW_NO_VERIFY=1 をコマンドに付けて実行してください。".to_string(),
                )];
            }
        }
        Vec::new()
    }
}
