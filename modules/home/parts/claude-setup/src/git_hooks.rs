//! `claude-setup git-hooks --bin <claude-hooks>`: グローバルの git hook を置く（ADR-0086。Linux は git-hooks.nix が置く）。
//!
//! ~/.config/git/hooks（Linux と同じ置き場）に、4 つの hook のファイル（claude-hooks の `git-hook <名前>` を exec するだけの
//! sh。Git for Windows は hook を同梱の sh で実行する）を書き、グローバルの core.hooksPath をそこに向ける。利用者がほかの
//! 置き場をグローバルの core.hooksPath にしていれば、黙って奪わずに止める（そこの hook が走らなくなる）

use std::path::{Path, PathBuf};
use std::process::Command;

/// 置く hook（中身は claude-hooks の src/git_hook.rs）
const HOOKS: [&str; 4] = [
    "pre-commit",
    "pre-merge-commit",
    "commit-msg",
    "post-rewrite",
];

pub fn run(home: &Path, bin: &str) -> Result<(), String> {
    let dir = home.join(".config/git/hooks");
    let current = global_hooks_path()?;
    if let Some(current) = &current
        && !same_dir(home, current, &dir)
    {
        return Err(format!(
            "❌ グローバルの core.hooksPath は {current} を指しています。ハーネスの git hook（{}）に向けると、そこの hook が走らなくなるので、ここで止めます\n   修正方法: そこの hook を移すか外してから、git config --global --unset core.hooksPath のあと、もう一度実行してください",
            dir.display()
        ));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("❌ {} を作れません（{e}）", dir.display()))?;
    for name in HOOKS {
        let path = dir.join(name);
        let text = format!(
            "#!/bin/sh\nexec {} git-hook {name} \"$@\"\n",
            sh_quote(&bin.replace('\\', "/"))
        );
        crate::sync::replace_with(&path, |tmp| {
            std::fs::write(tmp, &text)?;
            executable(tmp)
        })
        .map_err(|e| format!("❌ {} を書けません（{e}）", path.display()))?;
    }
    if current.is_none() {
        let value = dir.display().to_string().replace('\\', "/");
        let status = git()
            .args(["config", "--global", "core.hooksPath", &value])
            .status()
            .map_err(|e| format!("❌ git を起動できません（{e}）"))?;
        if !status.success() {
            return Err(format!(
                "❌ git config --global core.hooksPath {value} に失敗しました（上の git の出力を参照）"
            ));
        }
    }
    Ok(())
}

fn git() -> Command {
    Command::new("git")
}

/// グローバルの core.hooksPath（無ければ None）
fn global_hooks_path() -> Result<Option<String>, String> {
    let out = git()
        .args(["config", "--global", "--get", "core.hooksPath"])
        .output()
        .map_err(|e| format!("❌ git を起動できません（{e}）"))?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )),
        // 1 は「設定が無い」
        Some(1) => Ok(None),
        _ => Err(format!(
            "❌ グローバルの core.hooksPath を読めません（{}）",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// core.hooksPath の値（`~/` は git と同じくホームとして読む）が dir と同じ場所か
fn same_dir(home: &Path, value: &str, dir: &Path) -> bool {
    let path = match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value),
    };
    match (std::fs::canonicalize(&path), std::fs::canonicalize(dir)) {
        (Ok(a), Ok(b)) => a == b,
        _ => path == dir,
    }
}

/// sh の 1 つの引数として書く
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// 実行の権限を付ける（Windows には実行の bit が無く、Git for Windows はファイルがあれば hook として実行する）
fn executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
