//! `claude-setup sync [<ソースのリポジトリ>]`: ハーネスの .claude（CLAUDE.md、commands、skills、scripts）を ~/.claude に
//! 同期する（ADR-0084）。nixos-rebuild を待たずに作業中のスキルの変更を反映するためのコマンドで、home-manager の
//! activation も同じものを呼ぶ。
//!
//! - CLAUDE.md、skills、scripts: ソースが正。中身を比べ、違うファイルを上書きする（nix store のソースも ~/.claude 側も
//!   mtime が 1 になり、サイズと mtime では同じサイズの変更を見落とすため）
//! - commands: 既にあるファイルは上書きしない（利用者が手で直したもの）
//! - どこも、ソースに無いファイルを消さない（手で足したものを残す）
//! - 引数なし: ホームの下の nix/claude-harness（ハーネスの clone）から同期する。clone なら、既定のブランチで、同期する
//!   場所に未コミットの変更が無ければ `git pull --ff-only` で最新にしてから同期し、どれかに当たれば止める（古い版や
//!   書きかけで、rebuild の配った新しい版を黙って上書きしない）。clone でなければ（.git が無い）、そのまま同期する
//! - 引数あり: pull せず、その場所の今の中身を同期する（worktree で直しを試す、activation、pull できないとき）
//!
//! 外部のスキル（flake の input）は、rebuild の activation だけが配る。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 同期の場所（.claude の下の名前）と、既にあるファイルを上書きするか
const PLACES: [(&str, bool); 4] = [
    ("CLAUDE.md", true),
    ("commands", false),
    ("skills", true),
    ("scripts", true),
];

/// 止める理由（標準エラーに出す行）
pub struct Stop(pub Vec<String>);

fn stop(lines: &[String]) -> Stop {
    Stop(lines.to_vec())
}

pub fn run(home: &Path, source: Option<&Path>) -> Result<(), Stop> {
    let repo = source.map_or_else(|| home.join("nix/claude-harness"), Path::to_path_buf);
    if source.is_none() && repo.join(".git").exists() {
        pull(&repo)?;
    }
    let from = repo.join(".claude");
    if !from.is_dir() {
        return Err(stop(&[
            format!("❌ ソースが見つかりません: {}", from.display()),
            "Usage: claude-setup sync [source-repo]".to_string(),
        ]));
    }
    let to = home.join(".claude");
    fs::create_dir_all(&to).map_err(|e| io_stop(&to, &e))?;
    for (name, overwrite) in PLACES {
        let (src, dest) = (from.join(name), to.join(name));
        if !src.exists() {
            continue;
        }
        copy(&src, &dest, overwrite)?;
        let kept = if overwrite {
            ""
        } else {
            "（既存ファイルは保持）"
        };
        println!("✅ {name} synced{kept}");
    }
    println!(
        "🎉 Claude Code 設定を同期しました: {} → {}",
        from.display(),
        to.display()
    );
    Ok(())
}

fn io_stop(path: &Path, e: &std::io::Error) -> Stop {
    stop(&[format!("❌ {} を書けません（{e}）", path.display())])
}

/// git を、引き継いだリポジトリの環境変数（git の hook の中の GIT_DIR など）を外して作る
fn git(repo: &Path) -> Command {
    let mut command = Command::new("git");
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "--local-env-vars"])
        .output()
    {
        for name in String::from_utf8_lossy(&out.stdout).lines() {
            command.env_remove(name);
        }
    }
    command.arg("-C").arg(repo);
    command
}

/// git の標準出力（失敗なら None。git の標準エラーは利用者にそのまま見せる）
fn git_out(repo: &Path, args: &[&str]) -> Option<String> {
    let out = git(repo)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// 既定の clone を、既定のブランチの、コミットされた最新にする
fn pull(repo: &Path) -> Result<(), Stop> {
    let shown = repo.display();
    let as_is = format!("   今の中身をそのまま同期するなら: claude-setup sync {shown}");
    // git を実行できない、clone として読めない（safe.directory の拒否、壊れた .git）ときは、下の判定が既定値に
    // 落ちて誤った理由を示すので、先に止める
    if git_out(repo, &["rev-parse", "--git-dir"]).is_none() {
        return Err(stop(&[
            format!(
                "❌ {shown} を git のリポジトリとして読めません（上の出力を参照）。古い中身を同期しないよう、ここで止めます"
            ),
            as_is,
        ]));
    }
    let default = git_quiet(
        repo,
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
    )
    .map(|r| r.trim_start_matches("origin/").to_string())
    .filter(|r| !r.is_empty())
    .unwrap_or_else(|| "main".to_string());
    let branch = git_quiet(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
        .unwrap_or_else(|| "detached HEAD".to_string());
    if branch != default {
        return Err(stop(&[
            format!(
                "❌ {shown} は {default} でなく {branch} にあります。古い中身を同期しないよう、ここで止めます"
            ),
            format!(
                "   最新を入れるなら: git -C {shown} switch {default} のあと、もう一度 claude-setup sync"
            ),
            as_is,
        ]));
    }
    // 見るのは同期する場所だけ。追跡していない新しいファイルも、同期されるので数える
    let mut status = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    let places: Vec<String> = PLACES.iter().map(|(n, _)| format!(".claude/{n}")).collect();
    status.extend(places.iter().map(String::as_str));
    let Some(dirty) = git_out(repo, &status) else {
        return Err(stop(&[
            format!(
                "❌ {shown} の未コミットの変更を調べられません（上の出力を参照）。ここで止めます"
            ),
            as_is,
        ]));
    };
    if !dirty.is_empty() {
        return Err(stop(&[
            format!(
                "❌ {shown} の同期する場所に未コミットの変更があります。書きかけの中身を同期しないよう、ここで止めます"
            ),
            as_is,
        ]));
    }
    if git_out(repo, &["pull", "-q", "--ff-only"]).is_none() {
        return Err(stop(&[
            format!(
                "❌ {shown} を git pull --ff-only で最新にできません（上の git の出力を参照）。古い中身を同期しないよう、ここで止めます"
            ),
            as_is,
        ]));
    }
    let head = git_out(repo, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    println!("📥 {shown}: {default} {head}");
    Ok(())
}

/// git の標準出力（失敗なら None。標準エラーは見せない。無いことを確かめる問い合わせ）
fn git_quiet(repo: &Path, args: &[&str]) -> Option<String> {
    let out = git(repo).args(args).stderr(Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// src（ファイルかディレクトリ）を dest にコピーする。overwrite なら中身の違うファイルを上書きし、そうでなければ
/// 無いファイルだけを足す
fn copy(src: &Path, dest: &Path, overwrite: bool) -> Result<(), Stop> {
    if src.is_dir() {
        fs::create_dir_all(dest).map_err(|e| io_stop(dest, &e))?;
        writable(dest);
        let entries = fs::read_dir(src).map_err(|e| read_stop(src, &e))?;
        for entry in entries {
            let entry = entry.map_err(|e| read_stop(src, &e))?;
            copy(&entry.path(), &dest.join(entry.file_name()), overwrite)?;
        }
        return Ok(());
    }
    if dest.exists() && (!overwrite || same(src, dest)) {
        writable(dest);
        return Ok(());
    }
    // 読むだけのコピー（nix store のソースをコピーしたもの）の上にも書けるよう、消してからコピーする
    let _ = fs::remove_file(dest);
    fs::copy(src, dest).map_err(|e| io_stop(dest, &e))?;
    writable(dest);
    Ok(())
}

fn read_stop(path: &Path, e: &std::io::Error) -> Stop {
    stop(&[format!("❌ {} を読めません（{e}）", path.display())])
}

fn same(a: &Path, b: &Path) -> bool {
    matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// 持ち主の書き込みを足す（nix store からコピーした読むだけのファイルを、次の同期と利用者が書けるように）。実行の
/// bit などほかの bit は、コピーがコピーしたまま残す
fn writable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let mode = meta.permissions().mode();
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o200));
        }
    }
    #[cfg(not(unix))]
    {
        if let Ok(meta) = fs::metadata(path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

/// ホームのディレクトリ（HOME、Windows では USERPROFILE）
pub fn home() -> Option<PathBuf> {
    std::env::home_dir()
}
