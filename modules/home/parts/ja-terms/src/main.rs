//! ja-terms: 崩れた日本語の語の検査の CLI（ADR-0085）
//!
//! 使い方: ja-terms check [--dictionary <辞書>] <ファイル>...
//!
//! ファイルの全体に辞書を当て、textlint 15.7.1 の `--format unix` と同じ形
//! （`<ファイル>:<行>:<列>: <誤り> => <正しい語> [Error/prh]` の行と、末尾の空行と件数の行）で報告する。
//! `<ファイル>` は引数のとおり出す。拡張子 md は Markdown として、[`TEXT_EXTENSIONS`] はコードとテキストとして読み、
//! ほかの拡張子は検査できないとして止める（textlint は知らない拡張子のファイルを検査せずに通す）。
//! 辞書の既定は、ビルドに埋めた prh.yml。
//!
//! 終了コード: 0 = 崩れた語が無い、1 = 崩れた語がある、2 = 検査できなかった（使い方の誤り、読めないファイル、
//! 対象でない拡張子、辞書を読めない、prh と同じ意味で当てられない文）

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use ja_terms::Dictionary;
use ja_terms::report::{self, Syntax};

/// @textlint/text で読む拡張子（textlint.nix の codeExtensions と同じ）
const TEXT_EXTENSIONS: [&str; 9] = [
    "txt", "rs", "py", "sh", "nix", "toml", "yaml", "yml", "json",
];
const DICTIONARY: &str = include_str!("../prh.yml");
const USAGE: &str = "Usage: ja-terms check [--dictionary <辞書>] <ファイル>...";

/// 検査できなかった理由
struct Failure(String);

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(found) => ExitCode::from(u8::from(found)),
        Err(Failure(why)) => {
            eprintln!("ERROR: {why}");
            ExitCode::from(2)
        }
    }
}

/// 崩れた語があれば true
fn run(args: Vec<OsString>) -> Result<bool, Failure> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("check".as_ref()) {
        return Err(Failure(USAGE.to_string()));
    }
    let args: Vec<OsString> = args.collect();
    let (dictionary, args) = match args.as_slice() {
        [option, path, files @ ..] if option == "--dictionary" => (Dictionary::load(path), files),
        [option, ..] if option == "--dictionary" => return Err(Failure(USAGE.to_string())),
        files => (Dictionary::parse(DICTIONARY, "prh.yml"), files),
    };
    let dictionary = dictionary.map_err(|e| Failure(e.to_string()))?;
    if args.is_empty() {
        return Err(Failure(USAGE.to_string()));
    }
    // textlint と同じく、どのファイルも読めることを確かめてから検査する
    let files = args
        .iter()
        .map(|path| read(Path::new(path)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = String::new();
    let mut total = 0;
    for (path, (syntax, text)) in args.iter().zip(&files) {
        let path = path.to_string_lossy();
        let reports = report::check(&dictionary, text, *syntax)
            .map_err(|e| Failure(format!("{path}: {e}")))?;
        for r in &reports {
            out.push_str(&format!(
                "{path}:{}:{}: {} [Error/prh]\n",
                r.line, r.column, r.message
            ));
        }
        total += reports.len();
    }
    if total > 0 {
        let plural = if total == 1 { "" } else { "s" };
        out.push_str(&format!("\n{total} problem{plural}\n"));
    }
    match std::io::stdout().write_all(out.as_bytes()) {
        // 読み手が先に閉じても検査は済んでいるので、判定を返す
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(Failure(format!("報告を書けない: {e}")))
        }
        _ => Ok(total > 0),
    }
}

/// ファイルの読み方と中身。UTF-8 でない並びは、Node.js の読み込みと同じく U+FFFD にする
fn read(path: &Path) -> Result<(Syntax, String), Failure> {
    // ja-terms-files と同じく、名前に . があれば最後の . の後（.md の拡張子は md。Path::extension は . で始まる名前に
    // 拡張子を返さない）
    let name = path.file_name().map(|n| n.to_string_lossy());
    let extension = name
        .as_deref()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_lowercase());
    let syntax = match extension.as_deref() {
        Some("md") => Syntax::Markdown,
        Some(e) if TEXT_EXTENSIONS.contains(&e) => Syntax::Text,
        _ => {
            return Err(Failure(format!(
                "検査する拡張子ではありません: {}（対象: md {}）",
                path.display(),
                TEXT_EXTENSIONS.join(" ")
            )));
        }
    };
    let bytes = std::fs::read(path)
        .map_err(|e| Failure(format!("読めません: {}（{e}）", path.display())))?;
    Ok((syntax, String::from_utf8_lossy(&bytes).into_owned()))
}
