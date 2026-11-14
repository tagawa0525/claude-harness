//! `rename-branch.sh <new-name> [--remote]`: 今の feature ブランチの名前を変える。
//!
//! - main / master では変えない（1）
//! - リモートに同じ名前のブランチがあれば、--remote が無ければ何もせずに止まる（2）。リモートを書き換える操作なので、
//!   ユーザーの承認を得てから --remote を付けて実行し直す
//! - --remote のとき: 新しい名前を push（上流を設定）してから、前の名前をリモートから消す。ただし前の名前を head とする
//!   open の PR があれば、消すと PR が閉じるので止める（1）
//!
//! 出力: `RENAMED: <old> -> <new>`、`REMOTE: updated|none`

use crate::git::git;
use std::path::Path;
use std::process::Command;

const USAGE: &str = "Usage: rename-branch.sh <new-name> [--remote]";

fn usage() -> i32 {
    eprintln!("{USAGE}");
    1
}

/// git を標準入出力を引き継いで起動し、失敗すればその終了コード（そこで止める）
fn step(args: &[&str]) -> Result<(), i32> {
    match Command::new("git").args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("ERROR: git を起動できません: {e}");
            Err(1)
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    let mut new: Option<&str> = None;
    let mut remote_ok = false;
    for arg in args {
        match arg.as_str() {
            "--remote" => remote_ok = true,
            a if a.starts_with('-') => return usage(),
            a => {
                if new.is_some() {
                    return usage();
                }
                new = Some(a);
            }
        }
    }
    let Some(new) = new.filter(|n| !n.is_empty()) else {
        return usage();
    };
    match rename(new, remote_ok) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn rename(new: &str, remote_ok: bool) -> Result<(), i32> {
    let here = Path::new(".");
    let old = crate::git::current_branch(here);
    if old.is_empty() {
        eprintln!("ERROR: detached HEAD ではリネームできません");
        return Err(1);
    }
    if old == "main" || old == "master" {
        eprintln!("ERROR: {old} はリネームしません。feature ブランチで実行してください");
        return Err(1);
    }
    if old == new {
        eprintln!("ERROR: 現在のブランチ名と同じです: {new}");
        return Err(1);
    }
    let remote = git(here, &["config", &format!("branch.{old}.remote")])
        .unwrap_or_else(|| "origin".to_string());
    let has_remote = git(here, &["remote", "get-url", &remote]).is_some()
        && git(
            here,
            &[
                "ls-remote",
                "--heads",
                &remote,
                &format!("refs/heads/{old}"),
            ],
        )
        .is_some_and(|s| !s.is_empty());
    if has_remote && !remote_ok {
        outln!(
            "STOP: リモート {remote} に {old} が存在します。リネームするとリモートも更新されます。"
        );
        outln!("      続行するにはユーザーの承認を得てから --remote を付けて再実行してください");
        return Err(2);
    }
    if has_remote && crate::git::has_github_remote(here) {
        // 見つけたパスで起動する（Windows の .cmd の shim も。Command は拡張子の無い名前に .exe しか足さない）
        let gh = crate::staged_checks::find_on_path("gh").unwrap_or_else(|| "gh".into());
        let out = match Command::new(gh)
            .args(["pr", "view", &old, "--json", "state", "--jq", ".state"])
            .output()
        {
            Ok(out) => out,
            Err(_) => {
                eprintln!("ERROR: gh がないため {old} に open PR があるか確認できません");
                return Err(1);
            }
        };
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            if String::from_utf8_lossy(&out.stdout).trim() == "OPEN" {
                eprintln!(
                    "ERROR: {old} を head とする open PR があります。リモートのブランチを消すと PR が閉じるため、リネームしません"
                );
                return Err(1);
            }
        } else if !stderr.contains("no pull requests found") {
            eprintln!("ERROR: {old} の open PR の有無を確認できません: {stderr}");
            return Err(1);
        }
    }
    step(&["branch", "-m", &old, new])?;
    if has_remote {
        step(&["push", "-q", "-u", &remote, new])?;
        step(&["push", "-q", &remote, "--delete", &old])?;
        outln!("RENAMED: {old} -> {new}");
        outln!("REMOTE: updated");
    } else {
        outln!("RENAMED: {old} -> {new}");
        outln!("REMOTE: none");
    }
    Ok(())
}
