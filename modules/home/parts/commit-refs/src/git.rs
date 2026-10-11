//! git を呼ぶ道具。

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// 今の作業ツリーの直下
pub fn toplevel() -> Result<PathBuf, String> {
    let top = git(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    // git が足す改行だけを外す（ディレクトリ名の末尾の空白は名前の一部）
    Ok(PathBuf::from(OsStr::from_bytes(
        top.strip_suffix(b"\n").unwrap_or(&top),
    )))
}

/// `git args…` を `dir` で走らせ、成功なら標準出力を返す。
pub fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = run_git(dir, args)?;
    if !output.status.success() {
        return Err(git_failure(args, &output));
    }
    Ok(output.stdout)
}

/// `git args…` を `dir` で走らせる。終了コードの読み方は呼ぶ側が決める
pub fn run_git(dir: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git を起動できない: {e}"))
}

pub fn git_failure(args: &[&str], output: &Output) -> String {
    format!(
        "git {}（{}）: {}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}
