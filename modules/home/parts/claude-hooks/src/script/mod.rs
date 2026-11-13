//! `claude-hooks script <名前> [引数]...`: スキルが呼ぶ作業手順のスクリプトの中身（ADR-0087）。
//! `~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の同じ名前の sh は、これを exec するだけにする
//! （スキルの文と PreToolUse のルールはスクリプトの名前で呼び、見分けるので、名前と場所を変えない）

mod pr_url;
mod reply_to_comment;

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("reply-to-comment") => reply_to_comment::main(&args[1..]),
        _ => {
            eprintln!("Usage: claude-hooks script <名前> [引数]...");
            eprintln!("scripts: reply-to-comment");
            // claude-sync はスクリプトの sh だけを入れ替え、~/.claude/bin のバイナリは入れ直すまで前の版のまま
            eprintln!(
                "sh が正しい名前で呼んでも無いなら、claude-hooks が sh より古い。Linux は home-manager の rebuild、Windows は \
                 claude-harness の README の「Windows で使う」の cargo install で入れ直す"
            );
            2
        }
    }
}
