//! history-rewrite: 履歴の中の崩れた語を辞書で直し、文書と本文のコミットの番号を付け直す（ADR-0045、ADR-0088）。
//! 使い方と出力の形は main.rs の冒頭

pub mod glob;
pub mod run;
pub mod transform;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use transform::{Transformer, Words};

/// 止める理由。run は元のリポジトリに何も書かずに終わる
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop(pub String);

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `git -C repo <args>` の標準出力。失敗すれば、引数と終了コードと標準エラーを添えて止める
pub fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, Stop> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| Stop(format!("git を起動できない: {e}")))?;
    if !out.status.success() {
        return Err(Stop(format!(
            "git {}（終了コード {}）: {}",
            args.join(" "),
            out.status
                .code()
                .map_or("なし".to_string(), |c| c.to_string()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

/// transform: 作業ツリーのファイルの語を、run の第 1 段と同じ変換で直す（コミットはしない）。すべてのファイルの新しい
/// 中身を作ってから書く（変換が止まるファイルがあれば、どのファイルにも書かない）
pub fn transform_files(words: &Words, files: &[String]) -> Result<(), Stop> {
    let mut transformer = Transformer::new(words)?;
    let top = git(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    let top = String::from_utf8_lossy(&top)
        .trim_end_matches('\n')
        .to_string();
    // symlink を経たパスもリポジトリの中として比べるため、実体のパスにする
    let top = std::fs::canonicalize(&top).map_err(|e| Stop(format!("{top}: {e}")))?;
    let mut results: Vec<(PathBuf, String, Vec<u8>)> = Vec::new();
    let mut skipped: Vec<(String, &'static str)> = Vec::new();
    for name in files {
        let given = std::path::absolute(name).map_err(|e| Stop(format!("{name}: {e}")))?;
        // symlink のファイルそのものは見分けたいので、親のディレクトリだけをたどる
        let (Some(parent), Some(file_name)) = (given.parent(), given.file_name()) else {
            return Err(Stop(format!(
                "{name}: ファイルでない（無いか、ディレクトリ）"
            )));
        };
        let path = std::fs::canonicalize(parent)
            .map_err(|_| Stop(format!("{name}: ファイルでない（無いか、ディレクトリ）")))?
            .join(file_name);
        let Ok(relative) = path.strip_prefix(&top) else {
            return Err(Stop(format!("{name}: リポジトリ {} の外", top.display())));
        };
        let relative: String = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let meta = std::fs::symlink_metadata(&path);
        if meta.as_ref().is_ok_and(|m| m.file_type().is_symlink()) {
            // 履歴の書き換えと同じく、symlink は変えない（先のファイルは、それ自身のパスで直す）
            skipped.push((relative, "symlink"));
            continue;
        }
        if !meta.as_ref().is_ok_and(|m| m.is_file()) {
            return Err(Stop(format!(
                "{name}: ファイルでない（無いか、ディレクトリ）"
            )));
        }
        let data = std::fs::read(&path).map_err(|e| Stop(format!("{name}: 読めない: {e}")))?;
        if let Some(reason) = transformer.skipped(relative.as_bytes(), &data) {
            skipped.push((relative, reason));
            continue;
        }
        if let Some(new) = transformer.blob(relative.as_bytes(), &data)? {
            results.push((path, relative, new));
        }
    }
    let mut replaced = Vec::new();
    for (path, relative, new) in results {
        std::fs::write(&path, new).map_err(|e| Stop(format!("{relative}: 書けない: {e}")))?;
        replaced.push(relative);
    }
    for (relative, reason) in &skipped {
        println!("SKIPPED: {relative} ({reason})");
    }
    for relative in &replaced {
        println!("REPLACED: {relative}");
    }
    println!("FILES: {}", replaced.len());
    Ok(())
}
