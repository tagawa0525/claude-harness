//! history-rewrite: 履歴の中の崩れた語を辞書で直し、文書と本文のコミットの番号を付け直す（ADR-0045、ADR-0088）。
//!
//! Usage:
//!   history-rewrite transform [語の指定] FILE...
//!       作業ツリーのファイルの語を、run の第 1 段と同じ変換で直す（コミットはしない）
//!
//! 語の指定:
//!   --dictionary FILE   辞書（prh の形）。省くと ja-terms の辞書（ADR-0014。ビルドに埋めたもの）。重ねて渡せる
//!   --extra-rules FILE  辞書に足す規則（prh の形。辞書に無い 1 回限りの置き換え）。重ねて渡せる
//!   --exclude GLOB      直さないパス（リポジトリの直下から。Python の PurePosixPath.full_match の書き方で、** が
//!                       ディレクトリをまたぐ）。重ねて渡せる。コミットとタグの本文には当てない
//!
//! transform の出力:
//!   SKIPPED: <パス> (<理由>)  読まなかったファイルごと（symlink、excluded、binary、not UTF-8）
//!   REPLACED: <パス>    直したファイルごと
//!   FILES: <直したファイルの数>
//!
//! 終了コード: 0 = 成功、1 = 止めたか失敗、2 = 使い方の誤り

use history_rewrite::{Words, transform_files};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: history-rewrite transform [--dictionary FILE]... [--extra-rules FILE]... [--exclude GLOB]... FILE...";

enum Cli {
    Transform(Words, Vec<String>),
}

/// `--name value` と `--name=value` を受ける。使い方の誤りは None
fn parse(args: &[String]) -> Option<Cli> {
    let (command, rest) = args.split_first()?;
    let mut words = Words::default();
    let mut files = Vec::new();
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        if !arg.starts_with("--") {
            files.push(arg.clone());
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let value = match inline {
            Some(v) => v,
            None => it.next()?.clone(),
        };
        match name {
            "--dictionary" => words.dictionaries.push(PathBuf::from(value)),
            "--extra-rules" => words.extra_rules.push(PathBuf::from(value)),
            "--exclude" => words.excludes.push(value),
            _ => return None,
        }
    }
    match command.as_str() {
        "transform" if !files.is_empty() => Some(Cli::Transform(words, files)),
        _ => None,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cli) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match cli {
        Cli::Transform(words, files) => transform_files(&words, &files),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}
