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
//! 書き換える前に止めたとき（対応表の誤り、文書の語が複数の旧に当たる（曖昧）、文書の語が書き換えで消えた
//! コミットを指す、読めないファイル、git の失敗）は、ERROR: の最後に「何も書き換えていない」と出す。書き込みの途中で
//! 失敗したときは、書き換えたファイルを出す。
//!
//! 終了コード: 0 = 成功、1 = 止めたか失敗、2 = 使い方の誤り

use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod git;
mod remap;

const USAGE: &str = "Usage: commit-refs remap [MAP]";

fn main() -> ExitCode {
    // 対応表のパスは UTF-8 とは限らないので、OsString のまま読む
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let map_path = match args.as_slice() {
        [cmd] if cmd == "remap" => None,
        [cmd, path] if cmd == "remap" && !path.as_bytes().starts_with(b"-") => {
            Some(PathBuf::from(path))
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match remap::run(map_path.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            for e in errors {
                eprintln!("ERROR: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

/// 対応表をファイル（省略すると標準入力）から読む
fn read_map(map_path: Option<&Path>) -> Result<String, Vec<String>> {
    match map_path {
        Some(path) => fs::read_to_string(path)
            .map_err(|e| vec![format!("対応表 {} を読めない: {e}", path.display())]),
        None => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| vec![format!("標準入力の対応表を読めない: {e}")])?;
            Ok(text)
        }
    }
}
