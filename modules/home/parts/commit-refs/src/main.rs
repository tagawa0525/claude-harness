//! commit-refs: 履歴の書き換えで変わったコミットの番号を、文書の中で付け直す（ADR-0012）。
//!
//! Usage:
//!   commit-refs remap [MAP]  作業ツリーの追跡しているテキストのファイルの中の番号を、対応表 MAP（省略すると
//!                            標準入力）で付け直す。コミットはしない。対応表の形は commit_refs::Map::parse
//!
//! remap の出力（KEY: value）:
//!   REPLACED: <パス> <件数>   置き換えたファイルごと（パスはリポジトリの直下から）
//!   FILES: <置き換えたファイルの数>
//!   REFS: <置き換えた語の数>
//! 対応表の行が読めない、同じ旧に別々の新が対応する、文書の語が複数の旧に当たる（曖昧）、文書の語が書き換えで
//! 消えたコミットを指す、のどれかなら、何も書き換えずに ERROR: を出す。
//!
//! 終了コード: 0 = 成功、1 = 止めたか失敗、2 = 使い方の誤り

use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use commit_refs::Map;

const USAGE: &str = "Usage: commit-refs remap [MAP]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let map_path = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["remap"] => None,
        ["remap", path] if !path.starts_with('-') => Some(PathBuf::from(path)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match remap(map_path.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            for e in errors {
                eprintln!("ERROR: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn remap(map_path: Option<&Path>) -> Result<(), Vec<String>> {
    let text = match map_path {
        Some(path) => fs::read_to_string(path)
            .map_err(|e| vec![format!("対応表 {} を読めない: {e}", path.display())])?,
        None => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| vec![format!("標準入力の対応表を読めない: {e}")])?;
            text
        }
    };
    let map = Map::parse(&text)?;

    let top = git(Path::new("."), &["rev-parse", "--show-toplevel"]).map_err(|e| vec![e])?;
    let top = PathBuf::from(OsStr::from_bytes(top.trim_ascii_end()));
    let paths = candidate_paths(&top).map_err(|e| vec![e])?;

    let results = match collect(&top, &paths, &map) {
        Ok(results) => results,
        Err(mut errors) => {
            errors.push("何も書き換えていない".to_string());
            return Err(errors);
        }
    };

    let mut out = std::io::stdout().lock();
    let mut refs = 0;
    let mut written: Vec<String> = Vec::new();
    for (path, file, new_data, count) in &results {
        if let Err(e) = fs::write(file, new_data) {
            let done = if written.is_empty() {
                "なし".to_string()
            } else {
                written.join(", ")
            };
            return Err(vec![
                format!("{}: 書けない: {e}", String::from_utf8_lossy(path)),
                format!("書き換えたファイル: {done}"),
            ]);
        }
        written.push(String::from_utf8_lossy(path).into_owned());
        // パスはバイト列のまま出す（UTF-8 とは限らない）
        out.write_all(b"REPLACED: ")
            .and_then(|()| out.write_all(path))
            .and_then(|()| writeln!(out, " {count}"))
            .map_err(|e| vec![format!("標準出力に書けない: {e}")])?;
        refs += count;
    }
    writeln!(out, "FILES: {}\nREFS: {refs}", results.len())
        .map_err(|e| vec![format!("標準出力に書けない: {e}")])?;
    Ok(())
}

/// 置き換えるファイルごとに、リポジトリの直下からのパス、ファイル、新しい内容、置き換えた語の数
type Replacement<'a> = (&'a [u8], PathBuf, Vec<u8>, usize);

/// 書き換える前に、すべてのファイルの新しい内容を作る。止める理由が 1 つでもあれば、すべての理由を返す
fn collect<'a>(
    top: &Path,
    paths: &'a [Vec<u8>],
    map: &Map,
) -> Result<Vec<Replacement<'a>>, Vec<String>> {
    let mut errors = Vec::new();
    let mut results = Vec::new();
    for path in paths {
        let file = top.join(OsStr::from_bytes(path));
        // 追跡しているシンボリックリンクは、書くとリンク先を変えてしまうので飛ばす
        // （リンク先が追跡しているファイルなら、そちらとして置き換わる）
        let meta =
            fs::symlink_metadata(&file).map_err(|e| vec![format!("{}: {e}", file.display())])?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let data = fs::read(&file).map_err(|e| vec![format!("{}: {e}", file.display())])?;
        let label = String::from_utf8_lossy(path);
        let (new_data, count) = map.replace(&label, &data, &mut errors);
        if count > 0 {
            results.push((path.as_slice(), file, new_data, count));
        }
    }
    if errors.is_empty() {
        Ok(results)
    } else {
        Err(errors)
    }
}

/// 7 桁の 16 進を含む、追跡しているテキストのファイル（`-I` でバイナリを除く）の、リポジトリの直下からのパス
fn candidate_paths(top: &Path) -> Result<Vec<Vec<u8>>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["grep", "-z", "-l", "-I", "-E", "[0-9a-f]{7}"])
        .output()
        .map_err(|e| format!("git を起動できない: {e}"))?;
    // git grep は一致が無いと 1 を返すので、それだけを 0 件として扱う
    match output.status.code() {
        Some(0) => Ok(output
            .stdout
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect()),
        Some(1) => Ok(Vec::new()),
        _ => Err(format!(
            "git grep が失敗した（{}）: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

/// `git args…` を `dir` で走らせ、成功なら標準出力を返す。
fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git を起動できない: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}
