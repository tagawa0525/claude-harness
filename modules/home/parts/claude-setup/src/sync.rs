//! `claude-setup sync [<ソースのリポジトリ>]`: ハーネスの .claude（CLAUDE.md、commands、skills、scripts）を ~/.claude に
//! 同期する（ADR-0084）。nixos-rebuild を待たずに作業中のスキルの変更を反映するためのコマンドで、home-manager の
//! activation も同じものを呼ぶ。
//!
//! - CLAUDE.md、skills、scripts: ソースが正。中身を比べ、違うファイルを上書きする（nix store のソースも ~/.claude 側も
//!   mtime が 1 になり、サイズと mtime では同じサイズの変更を見落とすため。比べた案は
//!   <https://github.com/tagawa0525/nixfiles/issues/566>）
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
use std::sync::OnceLock;

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
    // 空の引数は引数なしと同じ（空の変数を渡した claude-sync "$REPO" で、今のディレクトリの .claude を同期しない）
    let source = source.filter(|s| !s.as_os_str().is_empty());
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
        // ソースに無いファイル（前の版の nix store から読むだけでコピーしたもの）も、利用者と次の同期が書けるようにする
        writable_tree(&dest);
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

/// git がリポジトリを指すのに読む環境変数（git rev-parse --local-env-vars。1 度だけ聞く）
fn local_env_vars() -> &'static [String] {
    static VARS: OnceLock<Vec<String>> = OnceLock::new();
    VARS.get_or_init(|| {
        Command::new("git")
            .args(["rev-parse", "--local-env-vars"])
            .output()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// git を、引き継いだリポジトリの環境変数（git の hook の中の GIT_DIR など）を外して作る
fn git(repo: &Path) -> Command {
    let mut command = Command::new("git");
    for name in local_env_vars() {
        command.env_remove(name);
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
    let as_is = format!(
        "   今の中身をそのまま同期するなら: claude-sync {shown}（Windows では claude-setup sync {shown}）"
    );
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
    .map(|r| r.strip_prefix("origin/").unwrap_or(&r).to_string())
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
                "   最新を入れるなら: git -C {shown} switch {default} のあと、もう一度 claude-sync（Windows では claude-setup sync）"
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

/// src（ファイル、ディレクトリ、symlink）を dest にコピーする。overwrite なら中身の違うファイルを上書きし、そうで
/// なければ無いファイルだけを足す。symlink はリンクのままコピーする（rsync -a と同じ。辿ると、親を指すリンクで
/// 止まらない）
fn copy(src: &Path, dest: &Path, overwrite: bool) -> Result<(), Stop> {
    let meta = fs::symlink_metadata(src).map_err(|e| read_stop(src, &e))?;
    if meta.file_type().is_symlink() {
        return copy_link(src, dest, overwrite);
    }
    let dest_meta = fs::symlink_metadata(dest).ok();
    let dest_is_link = dest_meta
        .as_ref()
        .is_some_and(|m| m.file_type().is_symlink());
    if meta.is_dir() {
        // ~/.claude の側の symlink は辿らない（利用者の作業の場所に書かない）。上書きする場所では、ソースのとおりの
        // ディレクトリに置き換え、既存を残す場所（commands）では触らない
        if dest_is_link {
            if !overwrite {
                return Ok(());
            }
            fs::remove_file(dest).map_err(|e| io_stop(dest, &e))?;
        }
        fs::create_dir_all(dest).map_err(|e| io_stop(dest, &e))?;
        let entries = fs::read_dir(src).map_err(|e| read_stop(src, &e))?;
        for entry in entries {
            let entry = entry.map_err(|e| read_stop(src, &e))?;
            copy(&entry.path(), &dest.join(entry.file_name()), overwrite)?;
        }
        return Ok(());
    }
    if dest_meta.is_some() && !overwrite {
        return Ok(());
    }
    // 行き先が symlink なら、中身が同じでもソースのとおりのファイルに置き換える
    if dest_meta.is_some() && !dest_is_link && same(src, dest) {
        same_mode(&meta, dest);
        return Ok(());
    }
    // 隣の一時のファイルに書いてから名前を付け替える（同期の間に読むほかのセッションに、消えたファイルや書きかけの
    // ファイルを見せない。書けなければ前の版を残す）
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dest.with_file_name(format!(".{name}.claude-setup-{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let copied = fs::copy(src, &tmp).and_then(|_| {
        writable(&tmp);
        fs::rename(&tmp, dest)
    });
    if let Err(e) = copied {
        let _ = fs::remove_file(&tmp);
        return Err(io_stop(dest, &e));
    }
    Ok(())
}

/// 中身の同じファイルの mode を、ソースに合わせる（rsync -a と同じ。実行の bit だけを足したコミット）。持ち主の
/// 書き込みは足したままにする
fn same_mode(src: &fs::Metadata, dest: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let want = src.permissions().mode() | 0o200;
        if fs::metadata(dest).is_ok_and(|m| m.permissions().mode() & 0o7777 != want & 0o7777) {
            let _ = fs::set_permissions(dest, fs::Permissions::from_mode(want));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (src, dest);
    }
}

/// symlink をリンクのままコピーする（Windows はリンクを作るのに権限が要るので、ファイルを指すリンクは行き先の
/// 中身をコピーし、ディレクトリを指すリンクは飛ばす）
fn copy_link(src: &Path, dest: &Path, overwrite: bool) -> Result<(), Stop> {
    let target = fs::read_link(src).map_err(|e| read_stop(src, &e))?;
    if let Ok(existing) = fs::read_link(dest) {
        if !overwrite || existing == target {
            return Ok(());
        }
    } else if fs::symlink_metadata(dest).is_ok() && !overwrite {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let _ = fs::remove_file(dest);
        std::os::unix::fs::symlink(&target, dest).map_err(|e| io_stop(dest, &e))
    }
    #[cfg(not(unix))]
    {
        if fs::metadata(src).is_ok_and(|m| m.is_dir()) {
            return Ok(());
        }
        let _ = fs::remove_file(dest);
        fs::copy(src, dest)
            .map(|_| ())
            .map_err(|e| io_stop(dest, &e))
    }
}

/// dest の下の、symlink でないファイルとディレクトリのすべてに、持ち主の書き込みを足す（chmod -R u+w）
fn writable_tree(dest: &Path) {
    let Ok(meta) = fs::symlink_metadata(dest) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    writable(dest);
    if meta.is_dir()
        && let Ok(entries) = fs::read_dir(dest)
    {
        for entry in entries.flatten() {
            writable_tree(&entry.path());
        }
    }
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

/// ホームのディレクトリ（HOME、Windows では USERPROFILE。Unix で HOME が空なら passwd のホーム）。空のパスは
/// 無いものとする（今のディレクトリの .claude に同期しない）
pub fn home() -> Option<PathBuf> {
    std::env::home_dir().filter(|p| !p.as_os_str().is_empty())
}
