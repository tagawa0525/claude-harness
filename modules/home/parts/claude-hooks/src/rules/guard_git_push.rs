//! guard-git-push: 保護対象への git push を止める
//!
//! - main / master への直接 push（PR を経由する）
//! - force push（--force / -f / +refspec）。--force-with-lease は feature branch に限り許可
//!   （open PR があっても可。origin/main にリベースしてからマージコミットする運用のため）
//! - --all / --mirror（main を含む全ブランチを押し出す）
//!
//! GitHub リモートがなければ PR フロー適用外で何もしない。
//! エスケープ: コマンドに `ALLOW_PROTECTED_PUSH=<理由>` を付ける（理由が会話ログに残る）

use std::path::Path;

use super::{Rule, escape, escape_hint};
use crate::git;
use crate::git_args::{git_flag, positionals};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct GuardGitPush;

fn deny(reason: &str) -> Finding {
    Finding::Deny(format!(
        "{reason}\n\n{}",
        escape_hint("ALLOW_PROTECTED_PUSH")
    ))
}

fn is_protected_ref(dst: &str) -> bool {
    let dst = dst.strip_prefix("refs/heads/").unwrap_or(dst);
    dst == "main" || dst == "master"
}

/// `git push` 1 つ分の検査。止める理由があれば返す
fn check_push(dir: &Path, args: &[Arg], has_error: bool) -> Option<String> {
    // --- force push ---
    // -f / --force（単独でも -fu のようなクラスタでも）。--force-with-lease / --force-if-includes は対象外
    if git_flag("push", args, Some('f'), &["--force"]) {
        return Some("force push は禁止です（レビュー履歴を保持するため）。feature branch で履歴を書き換える必要がある場合は --force-with-lease を使ってください".to_string());
    }
    if git_flag("push", args, None, &["--all", "--mirror"]) {
        return Some(
            "--all / --mirror は main を含む全ブランチを push するため禁止です".to_string(),
        );
    }
    if has_error {
        let raw: Vec<&str> = args.iter().map(|a| a.raw.as_str()).collect();
        return Some(format!(
            "git push の引数を解析できません（クォートが不整合）: {}",
            raw.join(" ")
        ));
    }

    // 位置引数（オプションとその値を除く）。1 つ目は remote、2 つ目以降が refspec
    let positional: Vec<&str> = positionals("push", args)
        .into_iter()
        .map(|a| a.text.as_str())
        .collect();
    let refspecs: &[&str] = if positional.len() > 1 {
        &positional[1..]
    } else {
        &[]
    };

    if refspecs.is_empty() {
        // refspec なし → 現在のブランチ（upstream 名が異なる場合は upstream で判定）
        let current = git::current_branch(dir);
        let upstream = git::git(
            dir,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .unwrap_or_default();
        let upstream = upstream
            .split_once('/')
            .map(|(_, b)| b)
            .unwrap_or(&upstream);
        if is_protected_ref(&current) || is_protected_ref(upstream) {
            return Some(format!(
                "main / master への直接 push は禁止です（現在のブランチ: {current}）。feature branch から PR を作成してください: /gh-pr-create"
            ));
        }
        return None;
    }
    for rs in refspecs {
        if let Some(_forced) = rs.strip_prefix('+') {
            return Some(format!("+refspec による force push は禁止です: {rs}"));
        }
        // src:dst → dst、単独なら全体。`origin HEAD` は現在ブランチ、`origin :branch`（削除）は対象外
        let dst = rs.split_once(':').map(|(_, d)| d).unwrap_or(rs);
        let dst = if dst == "HEAD" {
            git::current_branch(dir)
        } else if dst.is_empty() {
            continue;
        } else {
            dst.to_string()
        };
        if is_protected_ref(&dst) {
            return Some(format!(
                "main / master への直接 push は禁止です（refspec: {rs}）。feature branch から PR を作成してください: /gh-pr-create"
            ));
        }
    }
    None
}

impl Rule for GuardGitPush {
    fn name(&self) -> &'static str {
        "guard-git-push"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if let Some(findings) = escape(shell, "ALLOW_PROTECTED_PUSH") {
            return findings;
        }
        // コマンド中の全ての git push を検査する（2 つ目以降に保護対象が来る形を見逃さない）
        for cmd in shell.commands() {
            let Some(("push", args)) = cmd.git_subcommand() else {
                continue;
            };
            let dir = shell.target_dir(cmd, &input.cwd);
            if !git::has_github_remote(&dir) {
                continue;
            }
            if let Some(reason) = check_push(&dir, args, cmd.has_error) {
                return vec![deny(&reason)];
            }
        }
        Vec::new()
    }
}
