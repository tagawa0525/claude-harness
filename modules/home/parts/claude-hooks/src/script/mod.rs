//! `claude-hooks script <名前> [引数]...`: スキルが呼ぶ作業手順のスクリプトの中身（ADR-0087）。
//! `~/.claude/scripts/` と `~/.claude/skills/gh-pr-review/scripts/` の同じ名前の sh は、これを exec するだけにする
//! （スキルの文と PreToolUse のルールはスクリプトの名前で呼び、見分けるので、名前と場所を変えない）

mod pr_url;
mod reply_to_comment;

use std::process::Command;

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("reply-to-comment") => reply_to_comment::main(&args[1..]),
        _ => {
            eprintln!("Usage: claude-hooks script <名前> [引数]...");
            eprintln!("scripts: reply-to-comment");
            2
        }
    }
}

/// gh を、対象を pr のリポジトリにして起動し、標準入出力を引き継いで終了コードを返す。GH_HOST も固定するのは、
/// gh api が GH_REPO のホストより環境の GH_HOST を使うため（GH_REPO=github.com/<owner>/<repo> でも GH_HOST の
/// ホストへつなぐことを 2026-10-12 に確かめた）
fn gh(pr: &pr_url::Pr, args: &[&str]) -> i32 {
    match Command::new("gh")
        .args(args)
        .env("GH_REPO", &pr.repo)
        .env("GH_HOST", "github.com")
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("ERROR: gh を起動できません（PATH に gh が無い）: {e}");
            1
        }
    }
}
