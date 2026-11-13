//! ja-terms: 崩れた日本語の語の検査の CLI（ADR-0085）
//!
//! 使い方:
//!
//! - `ja-terms check [--dictionary <辞書>] <ファイル>...`: ファイルの全体に辞書を当て、textlint 15.7.1 の
//!   `--format unix` と同じ形（`<ファイル>:<行>:<列>: <誤り> => <正しい語> [Error/prh]` の行と、末尾の空行と件数の行）で
//!   報告する。`<ファイル>` は引数のとおり出す。拡張子 md は Markdown として、[`report::TEXT_EXTENSIONS`] はコードとテキストとして
//!   読み、ほかの拡張子は検査できないとして止める（textlint は知らない拡張子のファイルを検査せずに通す）
//! - `ja-terms message [--dictionary <辞書>]`: 標準入力のコミットのメッセージの本文（[`ja_terms::message::body`]）を
//!   Markdown として検査し、`<行>:<列>: <誤り> => <正しい語>` の行で報告する
//! - `ja-terms commit-msg [--dictionary <辞書>] <メッセージのファイル>`: git の commit-msg の hook と claude-hooks の
//!   git commit のルールが呼ぶ、件名の形と長さと本文の語の検査。理由は標準出力に出す
//! - `ja-terms staged [--dictionary <辞書>]`: グローバルの pre-commit がリポジトリの直下で呼ぶ、ステージした中身の
//!   検査（[`staged`]）
//!
//! 辞書の既定は、ビルドに埋めた prh.yml。
//!
//! 終了コード: 0 = 崩れた語が無い（commit-msg は規約に従う）、1 = 崩れた語がある（commit-msg は規約に反する）、
//! 2 = 検査できなかった（使い方の誤り、読めないファイル、対象でない拡張子、辞書を読めない、prh と同じ意味で当てられない文）

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

mod staged;

use ja_terms::Dictionary;
use ja_terms::message::{self, Subject};
use ja_terms::report::{self, Syntax, TEXT_EXTENSIONS};

const DICTIONARY: &str = include_str!("../prh.yml");
const USAGE: &str = "Usage: ja-terms check [--dictionary <辞書>] <ファイル>...
       ja-terms message [--dictionary <辞書>] < <メッセージ>
       ja-terms commit-msg [--dictionary <辞書>] <メッセージのファイル>
       ja-terms staged [--dictionary <辞書>]";

/// 検査できなかった理由
pub(crate) struct Failure(pub(crate) String);

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(Failure(why)) => {
            eprintln!("ERROR: {why}");
            ExitCode::from(2)
        }
    }
}

/// 終了コードを返す
fn run(args: Vec<OsString>) -> Result<u8, Failure> {
    let usage = || Failure(USAGE.to_string());
    let mut args = args.into_iter();
    let command = args.next().ok_or_else(usage)?;
    let args: Vec<OsString> = args.collect();
    let (dictionary, args) = match args.as_slice() {
        [option, path, rest @ ..] if option == "--dictionary" => (Dictionary::load(path), rest),
        [option, ..] if option == "--dictionary" => return Err(usage()),
        rest => (Dictionary::parse(DICTIONARY, "prh.yml"), rest),
    };
    let dictionary = dictionary.map_err(|e| Failure(e.to_string()))?;
    match (command.to_str(), args) {
        (Some("check"), [_, ..]) => check(&dictionary, args),
        (Some("message"), []) => {
            let mut input = Vec::new();
            std::io::stdin()
                .read_to_end(&mut input)
                .map_err(|e| Failure(format!("標準入力を読めない: {e}")))?;
            let reports = message_reports(&dictionary, &String::from_utf8_lossy(&input))?;
            print(&reports)?;
            Ok(u8::from(!reports.is_empty()))
        }
        (Some("commit-msg"), [path]) => commit_msg(&dictionary, Path::new(path)),
        (Some("staged"), []) => staged::run(&dictionary),
        _ => Err(usage()),
    }
}

/// 報告を標準出力に書く。読み手が先に閉じても検査は済んでいるので、誤りにしない
pub(crate) fn print(out: &str) -> Result<(), Failure> {
    match std::io::stdout().write_all(out.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(Failure(format!("報告を書けない: {e}")))
        }
        _ => Ok(()),
    }
}

/// ja-terms check。崩れた語があれば 1
fn check(dictionary: &Dictionary, args: &[OsString]) -> Result<u8, Failure> {
    // textlint と同じく、引数をパスの字面でファイルの集まりにし（同じファイルは最初の引数の名前で 1 度だけ）、
    // どのファイルも読めることを確かめてから検査する
    // ファイルは、textlint と同じく字面で外したパスで読む（OS に .. を任せると symlink を辿った先の親になる）
    let mut seen = std::collections::HashSet::new();
    let mut args_and_files = Vec::new();
    for arg in args {
        let resolved = resolve(Path::new(arg))?;
        if seen.insert(resolved.clone()) {
            args_and_files.push((arg, resolved));
        }
    }
    let files = args_and_files
        .iter()
        .map(|(arg, resolved)| read(Path::new(arg), resolved))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = String::new();
    let mut total = 0;
    for ((path, _), (syntax, text)) in args_and_files.iter().zip(&files) {
        let path = path.to_string_lossy();
        let reports = report::check(dictionary, text, *syntax)
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
    print(&out)?;
    Ok(u8::from(total > 0))
}

/// コミットのメッセージの本文の報告の行（`<行>:<列>: <誤り> => <正しい語>`）。崩れた語が無ければ空
fn message_reports(dictionary: &Dictionary, text: &str) -> Result<String, Failure> {
    let Some(body) = message::body(text) else {
        return Ok(String::new());
    };
    let reports =
        report::check(dictionary, &body, Syntax::Markdown).map_err(|e| Failure(e.to_string()))?;
    Ok(reports
        .iter()
        .map(|r| format!("{}:{}: {}\n", r.line, r.column, r.message))
        .collect())
}

/// ja-terms commit-msg。規約に反すれば 1、語を検査できなければ 2
fn commit_msg(dictionary: &Dictionary, path: &Path) -> Result<u8, Failure> {
    let bytes = std::fs::read(path)
        .map_err(|e| Failure(format!("読めません: {}（{e}）", path.display())))?;
    let text = String::from_utf8_lossy(&bytes);
    let subject = message::subject(&text);
    let mut out = String::new();
    if !message::is_exempt(subject) {
        match message::check_subject(subject) {
            Subject::Reject(why) => {
                print(&format!("{why}\n"))?;
                return Ok(1);
            }
            Subject::Pass(Some(warning)) => out.push_str(&format!("{warning}\n")),
            Subject::Pass(None) => {}
        }
    }
    let code = match message_reports(dictionary, &text) {
        Ok(reports) if reports.is_empty() => 0,
        Ok(reports) => {
            out.push_str(&reports);
            out.push_str(
                "❌ コミットの本文に崩れた日本語の語があります。上の「誤り => 正しい語」に直してください\n",
            );
            1
        }
        Err(Failure(why)) => {
            out.push_str(&format!("❌ 語の検査が失敗しました: {why}\n"));
            2
        }
    };
    print(&out)?;
    Ok(code)
}

/// Node.js の path.resolve と同じく、絶対パスにして . と .. を字面で外す（symlink は辿らない）
fn resolve(path: &Path) -> Result<std::path::PathBuf, Failure> {
    use std::path::Component;
    let absolute = std::path::absolute(path)
        .map_err(|e| Failure(format!("絶対パスにできません: {}（{e}）", path.display())))?;
    let mut out = std::path::PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// ファイルの読み方と中身。UTF-8 でない並びは、Node.js の読み込みと同じく U+FFFD にする
/// path は引数（名前と表示に使う）、file は読むパス
fn read(path: &Path, file: &Path) -> Result<(Syntax, String), Failure> {
    let name = path.file_name().map(|n| n.to_string_lossy());
    let Some(syntax) = name.as_deref().and_then(report::syntax_of) else {
        return Err(Failure(format!(
            "検査する拡張子ではありません: {}（対象: md {}）",
            path.display(),
            TEXT_EXTENSIONS.join(" ")
        )));
    };
    let bytes = std::fs::read(file)
        .map_err(|e| Failure(format!("読めません: {}（{e}）", path.display())))?;
    Ok((syntax, String::from_utf8_lossy(&bytes).into_owned()))
}
