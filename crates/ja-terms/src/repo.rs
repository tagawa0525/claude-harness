//! リポジトリの辞書の見つけ方（ADR-0096）。辞書は作業ツリーの直下の [`REPO_DICTIONARY`] で、今のディレクトリから根まで
//! 上って探さない（作業ツリーの外の `prh.yml` や、claude-harness の中の共通の辞書そのものを読まない）。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::DictionaryError;

/// リポジトリの辞書の、作業ツリーの直下からのパス。ja-terms のための辞書だと名前で分かるようにし（ほかの目的の
/// `prh.yml` を読まない）、形が prh の辞書だと分かるよう prh の名前を残す
pub const REPO_DICTIONARY: &str = ".ja-terms/prh.yml";

/// dir を含む git の作業ツリーの直下。作業ツリーの外（dir から根までに `.git` が無く、`GIT_DIR` も無い）なら None
///
/// 外かどうかを git の失敗の文言（言語の設定で変わる）で分けず、`.git` の有無で見る。作業ツリーの中と見たのに git が
/// 失敗すれば誤りにする（共通の辞書だけに黙ってすり替えない）
pub fn work_tree_top(dir: &Path) -> Result<Option<PathBuf>, DictionaryError> {
    // git と同じく symlink を辿った実体のパスで上る（symlink の置き場の上でなく、行き先の上の .git を見る）
    let absolute = std::fs::canonicalize(dir)
        .map_err(|e| DictionaryError(format!("{}: 実体のパスにできない: {e}", dir.display())))?;
    let has_git = |d: &Path| match d.join(".git").symlink_metadata() {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    };
    if std::env::var_os("GIT_DIR").is_none() && !absolute.ancestors().any(has_git) {
        return Ok(None);
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| DictionaryError(format!("リポジトリの辞書を探す git を起動できない: {e}")))?;
    if !out.status.success() {
        return Err(DictionaryError(format!(
            "リポジトリの辞書（{REPO_DICTIONARY}）を探す git rev-parse --show-toplevel が失敗した（{}、{}）: {}",
            dir.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(Some(path_from_git(out.stdout)))
}

/// dir を含む作業ツリーのリポジトリの辞書のパス。作業ツリーの外か、辞書が無ければ None
///
/// 有無は symlink を辿らずに見る（行き先の無い symlink を「無い」とせず、読む側で読めない辞書として止める）
pub fn dictionary_path(dir: &Path) -> Result<Option<PathBuf>, DictionaryError> {
    let Some(top) = work_tree_top(dir)? else {
        return Ok(None);
    };
    let path = top.join(REPO_DICTIONARY);
    match path.symlink_metadata() {
        Ok(_) => Ok(Some(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(DictionaryError(format!(
            "{}: 有無を確かめられない: {e}",
            path.display()
        ))),
    }
}

/// git rev-parse が出すパス（git が足す末尾の改行を除く）。Unix ではバイトのまま（UTF-8 でない名前のディレクトリも同じパスに
/// する）、Windows では git は UTF-8 で出す
pub fn path_from_git(mut out: Vec<u8>) -> PathBuf {
    // git は改行を 1 つ足す（名前が改行で終わるディレクトリもあるので、1 つだけ外す）
    if out.last() == Some(&b'\n') {
        out.pop();
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(out))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&out).into_owned())
    }
}
