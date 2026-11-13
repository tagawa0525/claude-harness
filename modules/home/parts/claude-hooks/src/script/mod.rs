//! `claude-hooks script <名前> [引数]...`: スキルが呼ぶ作業手順のスクリプトの中身（ADR-0087）。
//! `~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の同じ名前の sh は、これを exec するだけにする
//! （スキルの文と PreToolUse のルールはスクリプトの名前で呼び、見分けるので、名前と場所を変えない）

mod get_pr_info;
mod pr_url;
mod reply_to_comment;
mod reset_rounds;

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("get-pr-info") => get_pr_info::main(&args[1..]),
        Some("reply-to-comment") => reply_to_comment::main(&args[1..]),
        Some("reset-rounds") => reset_rounds::main(&args[1..]),
        _ => {
            eprintln!("Usage: claude-hooks script <名前> [引数]...");
            eprintln!("scripts: get-pr-info, reply-to-comment, reset-rounds");
            eprintln!("{}", crate::OLDER_THAN_CALLER);
            2
        }
    }
}
