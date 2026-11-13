//! `claude-hooks git-hook <名前> [引数]...`: グローバルの git hook の中身（ADR-0086）。hook のファイルは、これを呼ぶだけの
//! sh（Linux は modules/home/parts/git-hooks.nix、Windows は claude-setup が置く）。終了コードは git に返す
//! （0 以外は git がコミットを止める）。
//!
//! - commit-msg <メッセージのファイル>: ローカルの commit-msg があればそれに任せる。Claude Code のセッション
//!   （`CLAUDECODE=1`）のコミットで、fork でなければ、`ja-terms commit-msg` に渡す（Conventional Commits、件名の長さ、
//!   崩れた日本語の語。PreToolUse hook の check-commit-message が同じ検査を先に呼ぶ）。手で打つコミットの自由を残す
//!   ため、Claude Code の外のコミットは検査しない

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::git;
use crate::rules::ja_terms;

pub fn main(args: &[String]) -> i32 {
    match args {
        [name, rest @ ..] if name == "commit-msg" => commit_msg(rest),
        _ => {
            eprintln!("Usage: claude-hooks git-hook commit-msg <メッセージのファイル>");
            2
        }
    }
}

fn commit_msg(args: &[String]) -> i32 {
    let [file] = args else {
        eprintln!("Usage: claude-hooks git-hook commit-msg <メッセージのファイル>");
        return 2;
    };
    let dir = Path::new(".");
    if let Some(local) = local_hook(dir, "commit-msg") {
        return run_local(&local, args);
    }
    if std::env::var("CLAUDECODE").as_deref() != Ok("1") {
        return 0;
    }
    // 他人のプロジェクトの fork では、Conventional Commits も件名の長さも日本語の語の辞書もこちらの規約で、上流の
    // 慣習（"Fix socket transport when …" のような文）と衝突して上流向けのコミットを歪める
    if git::is_fork(dir) {
        println!(
            "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Conventional Commits、件名の長さ、日本語の語）を飛ばします"
        );
        return 0;
    }
    let Some(ja_terms) = ja_terms() else {
        eprintln!(
            "❌ ja-terms が見つからないので、コミットのメッセージを検査できません（Linux は claude-hooks の package.nix が CLAUDE_HOOKS_JA_TERMS を渡す）"
        );
        return 1;
    };
    status(
        Command::new(&ja_terms).arg("commit-msg").arg(file),
        &ja_terms,
    )
}

/// プロジェクトのローカルの hook。linked worktree でも共通の .git/hooks にある（--git-path hooks/… は core.hooksPath、
/// つまりこの hook 自身を指すので使わない）。Unix は実行の権限のあるファイル、Windows は実行の権限が無いので、
/// git と同じくファイルがあれば hook とみなす
fn local_hook(dir: &Path, name: &str) -> Option<PathBuf> {
    let common = git::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let path = Path::new(&common).join("hooks").join(name);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(path)
}

/// ローカルの hook を、git から受けた引数のまま実行する。Windows はスクリプトを直に起動できないので、Git for Windows の
/// sh に exec させる（sh の exec は shebang の行を読むので、sh 以外のスクリプトも同じく動く）
fn run_local(hook: &Path, args: &[String]) -> i32 {
    let mut cmd = if cfg!(windows) {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("exec \"$0\" \"$@\"").arg(hook);
        cmd
    } else {
        Command::new(hook)
    };
    cmd.args(args);
    status(&mut cmd, &hook.display().to_string())
}

/// 起動して、終了コードを返す（起動できなければ止める）
fn status(cmd: &mut Command, what: &str) -> i32 {
    match cmd.status() {
        // シグナルで終わったら（code が無い）止める
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("❌ {what} を起動できません（{e}）");
            1
        }
    }
}
