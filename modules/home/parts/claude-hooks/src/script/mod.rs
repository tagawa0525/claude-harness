//! `claude-hooks script <名前> [引数]...`: スキルが呼ぶ作業手順のスクリプトの中身（ADR-0087）。
//! `~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の同じ名前の sh は、これを exec するだけにする
//! （スキルの文と PreToolUse のルールはスクリプトの名前で呼び、見分けるので、名前と場所を変えない）

mod base_moved;
mod branch_topics;
mod decide_next;
mod get_latest_review;
mod get_pr_info;
mod get_review_comments;
mod gh_review_requests;
mod gh_wait_review;
mod git_merge_state;
mod post_merge_cleanup;
mod pr_url;
mod record_local_review;
mod rename_branch;
mod reply_to_comment;
mod request_rereview;
mod reset_rounds;
mod resolve_thread;
mod review_level;
mod worktree_add;

/// スクリプトの中身（引数を受けて終了コードを返す）
type Script = fn(&[String]) -> i32;

/// サブコマンドの名前と中身。名前の順に並べる（使い方の文の一覧もここから作る。スクリプトを足す PR どうしが、
/// 一覧の 1 行で衝突しないように）
const SCRIPTS: &[(&str, Script)] = &[
    ("base-moved", base_moved::main),
    ("branch-topics", branch_topics::main),
    ("decide-next", decide_next::main),
    ("get-latest-review", get_latest_review::main),
    ("get-pr-info", get_pr_info::main),
    ("get-review-comments", get_review_comments::main),
    ("gh-review-requests", gh_review_requests::main),
    ("gh-wait-review", gh_wait_review::main),
    ("git-merge-state", git_merge_state::main),
    ("post-merge-cleanup", post_merge_cleanup::main),
    ("record-local-review", record_local_review::main),
    ("rename-branch", rename_branch::main),
    ("reply-to-comment", reply_to_comment::main),
    ("request-rereview", request_rereview::main),
    ("reset-rounds", reset_rounds::main),
    ("resolve-thread", resolve_thread::main),
    ("review-level", review_level::main),
    ("worktree-add", worktree_add::main),
];

pub fn main(args: &[String]) -> i32 {
    let name = args.first().map(String::as_str);
    if let Some((_, run)) = SCRIPTS.iter().find(|(n, _)| Some(*n) == name) {
        return run(&args[1..]);
    }
    eprintln!("Usage: claude-hooks script <名前> [引数]...");
    let names: Vec<&str> = SCRIPTS.iter().map(|(n, _)| *n).collect();
    eprintln!("scripts: {}", names.join(", "));
    eprintln!("{}", crate::OLDER_THAN_CALLER);
    2
}

/// スキルとスクリプトの置き場（`~/.claude` か、リポジトリの `.claude`）。案内に書くスクリプトのパスを、呼んだ sh と
/// 同じツリーから引くため（デプロイ先の古い版を案内しない）。呼んだ sh の場所は `lib/claude-hooks-script.sh` が
/// CLAUDE_HOOKS_SCRIPT_DIR で渡す。`<root>/scripts` か `<root>/skills/<名前>/scripts` のどちらか。渡されなければ ~/.claude
pub fn claude_root() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("CLAUDE_HOOKS_SCRIPT_DIR").map(std::path::PathBuf::from) {
        let skills = dir.parent().and_then(|p| p.parent());
        if let Some(root) = skills
            .filter(|s| s.file_name().is_some_and(|n| n == "skills"))
            .and_then(|s| s.parent())
        {
            return root.to_path_buf();
        }
        if let Some(root) = dir.parent() {
            return root.to_path_buf();
        }
    }
    crate::shell::home().join(".claude")
}

#[cfg(test)]
mod tests {
    use super::SCRIPTS;

    #[test]
    fn the_scripts_are_in_the_order_of_their_names_without_duplicates() {
        // find は最初に一致したものを返すので、重複した名前は後の行を黙って隠す
        for pair in SCRIPTS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} と {}", pair[0].0, pair[1].0);
        }
    }
}
