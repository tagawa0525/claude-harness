//! guard-gh-api: 生の gh api で行ってはいけない操作を止める
//!
//! 1. Actions の権限設定（repos/.../actions/permissions 配下）への書き込み
//!    CI の権限エラーは、ワークフローの permissions: に必要な権限を最小限で宣言して直す。
//!    リポジトリ設定 default_workflow_permissions を write に緩めるのは回避策であり、
//!    全ワークフローの権限を広げるので禁止する。読み取り（GET）は通す
//! 2. GraphQL の resolveReviewThread / unresolveReviewThread
//!    スレッドの resolve は gh-pr-review の resolve-thread.sh 経由に固定する。スクリプトは
//!    人間のレビュアーが起こしたスレッドを既定で拒否するので、生の mutation を許すと迂回できてしまう。
//!    mutation 本文はヒアドキュメントで渡されることがあるため、引数ノードの原文（raw）で探す
//!
//! エスケープはない（どちらも Claude Code セッションから行う理由がない）

use std::sync::OnceLock;

use regex::Regex;

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell, has_flag, opt_value};

pub struct GuardGhApi;

fn resolve_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(^|[^A-Za-z])(un)?resolveReviewThread").expect("re"))
}

fn check_api(args: &[Arg]) -> Option<String> {
    if args.iter().any(|a| a.text.contains("actions/permissions")) {
        let method_writes = opt_value(args, &["-X", "--method"])
            .map(|v| {
                v.text
                    .trim_start_matches("-X=")
                    .trim_start_matches("--method=")
            })
            .is_some_and(|m| matches!(m, "PUT" | "PATCH" | "POST" | "DELETE"));
        // フィールドや入力ファイルを渡すと gh api は既定で POST になる
        let fields = has_flag(args, &["-f", "-F", "--field", "--raw-field", "--input"]);
        if method_writes || fields {
            return Some("リポジトリの Actions 権限設定（actions/permissions、default_workflow_permissions 等）は変更しません。全ワークフローの権限を広げる回避策になるためです。CI の権限エラーは、そのワークフローの permissions: に必要な権限を最小限で宣言して直してください（例: pull-requests: write）".to_string());
        }
    }
    if args.first().is_some_and(|a| a.text == "graphql")
        && args.iter().any(|a| resolve_re().is_match(&a.raw))
    {
        return Some("レビュースレッドの resolve / unresolve は生の GraphQL では行いません。~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh <pr_number> <comment_id> を使ってください（人間のレビュアーが起こしたスレッドは既定で resolve しない判定をスクリプトが行います）".to_string());
    }
    None
}

impl Rule for GuardGhApi {
    fn name(&self) -> &'static str {
        "guard-gh-api"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(args) = cmd.gh_subcommand(&["api"]) else {
                continue;
            };
            if let Some(reason) = check_api(args) {
                return vec![Finding::Deny(reason)];
            }
        }
        Vec::new()
    }
}
