//! `git-info.sh`: 今の Git の状態を一覧にする（git-info のスキルの集める部分と整える部分）。ブランチ、未コミットの
//! 変更、未プッシュのコミット、stash、worktree、関連の PR を出し、既定のブランチ（main / master）にマージ済みの
//! ブランチを持つ worktree を見つける。情報を集めるだけなので、個々のコマンドの失敗では止めず、代わりの表示にする

use std::process::Command;

/// git の標準出力（失敗すれば空）
fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_end_matches('\n')
                .to_string()
        })
        .unwrap_or_default()
}

/// 各行を字下げして出す。空なら「(なし)」
fn indent_or_none(text: &str) {
    if text.is_empty() {
        outln!("   (なし)");
    } else {
        for line in text.lines() {
            outln!("   {line}");
        }
    }
}

pub fn main(_args: &[String]) -> i32 {
    let ok = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("ERROR: git リポジトリではありません");
        return 1;
    }
    let mut branch = git(&["branch", "--show-current"]);
    if branch.is_empty() {
        branch = format!(
            "(detached HEAD: {})",
            git(&["rev-parse", "--short", "HEAD"])
        );
    }
    let upstream = git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]);
    outln!("📍 現在のブランチ: {branch}");
    if upstream.is_empty() {
        outln!("   上流: (未設定)");
    } else {
        let counts = git(&[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{upstream}"),
        ]);
        let mut words = counts.split_whitespace();
        let ahead = words.next().unwrap_or("?");
        let behind = words.next().unwrap_or("?");
        outln!("   上流: {upstream} (ahead {ahead} / behind {behind})");
    }

    outln!();
    outln!("📝 未コミット変更:");
    indent_or_none(&git(&["status", "--short"]));

    outln!();
    outln!("📤 未プッシュコミット:");
    if upstream.is_empty() {
        outln!("   (上流ブランチ未設定)");
    } else {
        indent_or_none(&git(&["log", &format!("{upstream}..HEAD"), "--oneline"]));
    }

    outln!();
    outln!("📦 stash:");
    indent_or_none(&git(&["stash", "list"]));

    outln!();
    outln!("🌳 worktree:");
    indent_or_none(&git(&["worktree", "list"]));

    outln!();
    outln!("🔗 関連PR:");
    if !git(&["remote", "-v"]).contains("github.com") {
        outln!("   (GitHub リモートなし)");
    } else {
        match crate::staged_checks::find_on_path("gh") {
            None => outln!("   (gh 未インストール)"),
            Some(gh) => {
                // gh pr status は標準エラーにも出すので、両方を順に並べる
                let text = Command::new(gh)
                    .args(["pr", "status"])
                    .output()
                    .map(|o| {
                        let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                        s.push_str(&String::from_utf8_lossy(&o.stderr));
                        s.trim_end_matches('\n').to_string()
                    })
                    .unwrap_or_default();
                indent_or_none(&text);
            }
        }
    }

    stale_worktrees();
    0
}

/// 既定のブランチにマージ済みのブランチを持つ linked worktree を知らせる
fn stale_worktrees() {
    let Some(default) = ["main", "master"].into_iter().find(|b| {
        Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{b}"),
            ])
            .status()
            .is_ok_and(|s| s.success())
    }) else {
        return;
    };
    let list = git(&["worktree", "list", "--porcelain"]);
    let main_root = list
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("worktree "))
        .unwrap_or("");
    let merged = git(&["branch", "--merged", default, "--format=%(refname:short)"]);
    let merged: Vec<&str> = merged.lines().collect();
    let mut stale: Vec<(String, String)> = Vec::new();
    let mut path = String::new();
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = p.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            let b = b.strip_prefix("refs/heads/").unwrap_or(b);
            if path != main_root && b != default && merged.contains(&b) {
                stale.push((path.clone(), b.to_string()));
            }
        }
    }
    if stale.is_empty() {
        return;
    }
    outln!();
    outln!("⚠️ マージ済みブランチのworktreeがあります:");
    outln!();
    for (path, branch) in &stale {
        outln!("- {path} ({branch}) - マージ済み");
    }
    outln!();
    outln!("削除するには:");
    for (_, branch) in &stale {
        outln!("  /git-worktree {branch} --remove");
    }
}
