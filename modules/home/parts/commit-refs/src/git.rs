//! git を呼ぶ道具。

use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

/// `git args…` を `dir` で走らせて `input` を渡し、成功なら標準出力を返す。入力は別のスレッドで書く（git が先に
/// 終わって書き込みが broken pipe になっても、git の標準エラーの理由を返すため）
pub fn git_stdin(dir: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git を起動できない: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    let written = writer.join().expect("writer thread");
    if !output.status.success() {
        return Err(git_failure(args, &output));
    }
    written.map_err(|e| format!("git {} に入力を渡せない: {e}", args.join(" ")))?;
    Ok(output.stdout)
}
