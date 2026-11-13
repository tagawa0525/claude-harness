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
    // 無いパスや相対のパス（git は hook を各リポジトリの直下で起動する）を 4 つの hook に書くと、その機械のすべての
    // git commit が止まる
    let runnable = std::fs::metadata(bin).is_ok_and(|m| m.is_file() && executable_bit(&m));
    if !Path::new(bin).is_absolute() || !runnable {
        return Err(format!(
            "❌ {bin} は絶対パスの実行できるファイルではありません。claude-hooks の実行ファイルの絶対パスを --bin に渡してください"
        ));
    }
    let dir = home.join(".config/git/hooks");
    // system の値も git は使い、global に書くと上書きしてしまうので、同じく奪わない
    if let Some(system) = hooks_path("--system")?
        && !same_dir(home, &system, &dir)
    {
        return Err(format!(
            "❌ system の core.hooksPath は {system} を指しています。global に書くと、そこの hook が走らなくなるので、ここで止めます\n   修正方法: system の設定を管理する人に、その hook の扱いを確かめてください（system の設定を読ませないなら GIT_CONFIG_NOSYSTEM=1）"
        ));
    }
    let current = hooks_path("--global")?;
    if let Some(current) = &current
        && !same_dir(home, current, &dir)
    {
        return Err(format!(
            "❌ グローバルの core.hooksPath は {current} を指しています。ハーネスの git hook（{}）に向けると、そこの hook が走らなくなるので、ここで止めます\n   修正方法: そこの hook を移すか外してから、git config --global --unset core.hooksPath のあと、もう一度実行してください",
            dir.display()
        ));
    }
    // 同じ置き場の、ハーネスのものでない hook（利用者が書いたもの）は、黙って上書きしない。ハーネスの hook のファイル
    // は、claude-hooks の置き場が変わっていても ` git-hook <名前> ` を含む
    // ハーネスの hook のファイルは、このコマンドが書く 2 行の形（claude-hooks の置き場は問わない）だけ。claude-hooks を
    // 呼ぶ利用者のラッパー、読めないファイル（UTF-8 でないものを含む）、symlink（Linux の home-manager が store に置く
    // もの）は、持ち主のものとして上書きしない。Git for Windows は `<名前>.exe` も hook として探す
    let foreign: Vec<&str> = HOOKS
        .into_iter()
        .filter(|name| {
            let path = dir.join(name);
            let script = match std::fs::symlink_metadata(&path) {
                Err(e) => e.kind() != std::io::ErrorKind::NotFound,
                Ok(meta) if meta.file_type().is_symlink() => true,
                Ok(_) => !std::fs::read_to_string(&path).is_ok_and(|text| generated(&text, name)),
            };
            script || (cfg!(windows) && dir.join(format!("{name}.exe")).exists())
        })
        .collect();
    if !foreign.is_empty() {
        return Err(format!(
            "❌ {} に、ハーネスのものでない hook があります（{}）。上書きすると走らなくなるので、ここで止めます\n   修正方法: その hook を移すか外してから、もう一度実行してください",
            dir.display(),
            foreign.join("、")
        ));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("❌ {} を作れません（{e}）", dir.display()))?;
    for name in HOOKS {
        let path = dir.join(name);
        let text = format!(
            "#!/bin/sh\nexec {} git-hook {name} \"$@\"\n",
            sh_quote(&sh_path(bin))
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

/// scope（--global、--system）の core.hooksPath（無ければ None）
fn hooks_path(scope: &str) -> Result<Option<String>, String> {
    // GIT_CONFIG_NOSYSTEM では git は system の設定を読まないが、--system を付けた git config は読むので、ここで外す
    if scope == "--system" && nosystem() {
        return Ok(None);
    }
    let out = git()
        // include した設定の値も git は使うので、--includes で読む（scope を付けると include を辿らない）
        .args(["config", scope, "--includes", "--get", "core.hooksPath"])
        .output()
        .map_err(|e| format!("❌ git を起動できません（{e}）"))?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )),
        // 1 は「設定が無い」
        Some(1) => Ok(None),
        _ => Err(format!(
            "❌ {scope} の core.hooksPath を読めません（{}）",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// core.hooksPath の値（`~/` は git と同じくホームとして読む）が dir と同じ場所か。git は `~` を HOME で読む（Windows で
/// HOME が USERPROFILE と違うこともある）ので、HOME があればそれを使う
fn same_dir(home: &Path, value: &str, dir: &Path) -> bool {
    let git_home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf());
    let path = match value.strip_prefix("~/") {
        Some(rest) => git_home.join(rest),
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

/// sh に書くパス。Windows のパスの `\` は sh が消すので `/` にする（Unix では `\` もファイル名の字なので変えない）
fn sh_path(bin: &str) -> String {
    if cfg!(windows) {
        bin.replace('\\', "/")
    } else {
        bin.to_string()
    }
}

/// GIT_CONFIG_NOSYSTEM が真か（git と同じく true、yes、on、1 を真とする）
fn nosystem() -> bool {
    std::env::var("GIT_CONFIG_NOSYSTEM")
        .is_ok_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// このコマンドが書いた hook のファイルの形か（claude-hooks の置き場は問わない）
fn generated(text: &str, name: &str) -> bool {
    let mut lines = text.lines();
    lines.next() == Some("#!/bin/sh")
        && lines.next().is_some_and(|line| {
            line.starts_with("exec '") && line.ends_with(&format!("' git-hook {name} \"$@\""))
        })
        && lines.next().is_none()
}

/// 実行の bit があるか（Windows には実行の bit が無い）
fn executable_bit(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}
