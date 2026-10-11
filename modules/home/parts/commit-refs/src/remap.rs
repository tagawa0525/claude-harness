//! remap: 作業ツリーの追跡しているテキストのファイルの中の番号を、対応表で付け直す（ADR-0012）。

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use commit_refs::Map;

use crate::{git, read_map};

pub fn run(map_path: Option<&Path>) -> Result<(), Vec<String>> {
    let results = match prepare(map_path) {
        Ok(results) => results,
        Err(mut errors) => {
            errors.push("何も書き換えていない".to_string());
            return Err(errors);
        }
    };

    // 書き終えてから出力する。出力の途中で失敗しても、ファイルは揃って書き換わっている
    let mut written: Vec<String> = Vec::new();
    for (path, file, new_data, _) in &results {
        if let Err(e) = replace_file(file, new_data) {
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
    }

    let mut report = Vec::new();
    let mut refs = 0;
    for (path, _, _, count) in &results {
        // パスはバイト列のまま出す（UTF-8 とは限らない）
        report.extend_from_slice(b"REPLACED: ");
        report.extend_from_slice(path);
        report.extend_from_slice(format!(" {count}\n").as_bytes());
        refs += count;
    }
    report.extend_from_slice(format!("FILES: {}\nREFS: {refs}\n", results.len()).as_bytes());
    std::io::stdout().lock().write_all(&report).map_err(|e| {
        vec![format!(
            "標準出力に書けない（ファイルはすべて書き換えた）: {e}"
        )]
    })
}

/// 対応表を読み、書き換える前に、すべてのファイルの新しい内容を作る
fn prepare(map_path: Option<&Path>) -> Result<Vec<Replacement>, Vec<String>> {
    let text = read_map(map_path)?;
    let map = Map::parse(&text)?;

    let top = git::toplevel().map_err(|e| vec![e])?;
    let (paths, mut problems) = candidate_paths(&top).map_err(|e| vec![e])?;
    match collect(&top, &paths, &map) {
        Ok(results) if problems.is_empty() => Ok(results),
        Ok(_) => Err(problems),
        Err(errors) => {
            problems.extend(errors);
            Err(problems)
        }
    }
}

/// `file` の中身を `data` にする。同じディレクトリの一時ファイルに書いて権限を写し、rename で置き換えるので、
/// 途中で失敗しても `file` は元のまま残る（fs::write は先に中身を消すので、書き込みの失敗でファイルが壊れる）
fn replace_file(file: &Path, data: &[u8]) -> std::io::Result<()> {
    let permissions = fs::metadata(file)?.permissions();
    // 一時ファイルの名前は元の名前を含めない（名前が NAME_MAX に近いと超える）。ぶつかったら番号を変える
    let mut attempt = 0u32;
    let (temp, mut out) = loop {
        let temp =
            file.with_file_name(format!(".commit-refs-{}-{attempt}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(out) => break (temp, out),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1
            }
            Err(e) => return Err(e),
        }
    };
    let result = out
        .write_all(data)
        .and_then(|()| out.sync_all())
        .and_then(|()| fs::set_permissions(&temp, permissions))
        .and_then(|()| fs::rename(&temp, file));
    if result.is_err() {
        // 作った一時ファイルは残さない（消せなくても、元の失敗の理由を返す）
        let _ = fs::remove_file(&temp);
    }
    result
}

/// 置き換えるファイルごとに、リポジトリの直下からのパス、ファイル、新しい内容、置き換えた語の数
type Replacement = (Vec<u8>, PathBuf, Vec<u8>, usize);

/// すべてのファイルの新しい内容を作る。止める理由が 1 つでもあれば、すべての理由を返す
fn collect(top: &Path, paths: &[Vec<u8>], map: &Map) -> Result<Vec<Replacement>, Vec<String>> {
    let mut errors = Vec::new();
    let mut results = Vec::new();
    for path in paths {
        let file = top.join(OsStr::from_bytes(path));
        let label = String::from_utf8_lossy(path);
        // 追跡しているシンボリックリンクは、書くとリンク先を変えてしまうので飛ばす
        // （リンク先が追跡しているファイルなら、そちらとして置き換わる）
        let data = match fs::symlink_metadata(&file) {
            Ok(meta) if meta.file_type().is_symlink() => continue,
            Ok(_) => fs::read(&file),
            Err(e) => Err(e),
        };
        let data = match data {
            Ok(data) => data,
            Err(e) => {
                errors.push(format!("{label}: 読めない: {e}"));
                continue;
            }
        };
        let (new_data, count) = map.replace(&label, &data, &mut errors);
        if count > 0 {
            results.push((path.clone(), file, new_data, count));
        }
    }
    if errors.is_empty() {
        Ok(results)
    } else {
        Err(errors)
    }
}

/// 7 桁の 16 進を含む、追跡しているテキストのファイル（`-I` でバイナリを除く）の、リポジトリの直下からのパスと、
/// git grep が読めなかったファイルの理由。git grep は読めないファイルを飛ばして標準エラーに出すだけで、終了コード
/// では分からないので、標準エラーの行を止める理由として返す（ほかの理由と一緒に出せるよう、ここでは止めない）
fn candidate_paths(top: &Path) -> Result<(Vec<Vec<u8>>, Vec<String>), String> {
    let args = ["grep", "-z", "-l", "-I", "-E", "[0-9a-f]{7}"];
    let output = git::run_git(top, &args)?;
    let problems = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("git grep: {}", line.trim()))
        .collect();
    // git grep は一致が無いと 1 を返すので、それだけを 0 件として扱う
    match output.status.code() {
        Some(0) => Ok((
            output
                .stdout
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(<[u8]>::to_vec)
                .collect(),
            problems,
        )),
        Some(1) => Ok((Vec::new(), problems)),
        _ => Err(git::git_failure(&args, &output)),
    }
}
