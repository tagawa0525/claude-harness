//! `claude-hooks script <名前> [引数]...`: スキルが呼ぶ作業手順のスクリプトの中身（ADR-0087）。
//! `~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の同じ名前の sh は、これを exec するだけにする
//! （スキルの文と PreToolUse のルールはスクリプトの名前で呼び、見分けるので、名前と場所を変えない）

mod get_latest_review;
mod get_pr_info;
mod get_review_comments;
mod gh_review_requests;
mod gh_wait_review;
mod pr_url;
mod record_local_review;
mod reply_to_comment;
mod request_rereview;
mod reset_rounds;
mod resolve_thread;

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("get-latest-review") => get_latest_review::main(&args[1..]),
        Some("get-pr-info") => get_pr_info::main(&args[1..]),
        Some("get-review-comments") => get_review_comments::main(&args[1..]),
        Some("gh-review-requests") => gh_review_requests::main(&args[1..]),
        Some("gh-wait-review") => gh_wait_review::main(&args[1..]),
        Some("record-local-review") => record_local_review::main(&args[1..]),
        Some("reply-to-comment") => reply_to_comment::main(&args[1..]),
        Some("request-rereview") => request_rereview::main(&args[1..]),
        Some("reset-rounds") => reset_rounds::main(&args[1..]),
        Some("resolve-thread") => resolve_thread::main(&args[1..]),
        _ => {
            eprintln!("Usage: claude-hooks script <名前> [引数]...");
            eprintln!(
                "scripts: get-latest-review, get-pr-info, get-review-comments, gh-review-requests, gh-wait-review, record-local-review, reply-to-comment, request-rereview, reset-rounds, resolve-thread"
            );
            eprintln!("{}", crate::OLDER_THAN_CALLER);
            2
        }
    }
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
