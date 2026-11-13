//! claude-setup: ハーネスを Claude Code の設定に配る設置のコマンド（ADR-0084）
//!
//! 使い方:
//!
//! - `claude-setup sync [<ソースのリポジトリ>]`: ハーネスの .claude を ~/.claude に同期する（[`sync`]）
//! - `claude-setup settings --static <JSON> [--pre-tool-use <バイナリ>] [--stop <バイナリ>] [--legacy <ファイル名>]...`:
//!   ~/.claude/settings.json に静的な設定を重ね、ハーネスの hook を登録する（[`settings`]）
//! - `claude-setup git-hooks --bin <claude-hooks>`: グローバルの git hook を置き、core.hooksPath を向ける（[`git_hooks`]）
//!
//! 終了コード: 0 = 済んだ、1 = 止めた（理由は標準エラー）、2 = 使い方の誤り

mod git_hooks;
mod settings;
mod sync;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "Usage: claude-setup sync [source-repo]
       claude-setup settings --static <json> [--pre-tool-use <binary>] [--stop <binary>] [--legacy <file-name>]...
       claude-setup git-hooks --bin <claude-hooks>";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let help = |a: &OsString| a == "--help" || a == "-h";
    if matches!(args.as_slice(), [h] if help(h)) || matches!(args.as_slice(), [_, h] if help(h)) {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let Some(home) = sync::home() else {
        eprintln!("❌ ホームのディレクトリが分かりません（HOME、Windows では USERPROFILE）");
        return ExitCode::from(1);
    };
    match args.split_first() {
        Some((command, rest)) if command == "sync" => {
            let source = match rest {
                [] => None,
                [source] => Some(Path::new(source)),
                _ => return usage(),
            };
            match sync::run(&home, source) {
                Ok(()) => ExitCode::SUCCESS,
                Err(sync::Stop(lines)) => fail(&lines),
            }
        }
        Some((command, rest)) if command == "settings" => {
            let Some(options) = settings_options(rest) else {
                return usage();
            };
            match settings::run(&home, &options) {
                Ok(()) => {
                    println!("✅ settings.json を更新しました");
                    ExitCode::SUCCESS
                }
                Err(line) => fail(&[line]),
            }
        }
        Some((command, rest)) if command == "git-hooks" => {
            let [flag, bin] = rest else {
                return usage();
            };
            if flag != "--bin" {
                return usage();
            }
            match git_hooks::run(&home, &bin.to_string_lossy()) {
                Ok(()) => {
                    println!("✅ git hook を置き、core.hooksPath を向けました");
                    ExitCode::SUCCESS
                }
                Err(line) => fail(&[line]),
            }
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn fail(lines: &[String]) -> ExitCode {
    for line in lines {
        eprintln!("{line}");
    }
    ExitCode::from(1)
}

/// settings の引数（--static は 1 つ要る。どのオプションも値を 1 つ取る）
fn settings_options(args: &[OsString]) -> Option<settings::Options> {
    let (mut statics, mut pre_tool_use, mut stop, mut legacy) = (None, None, None, Vec::new());
    let mut args = args.iter();
    while let Some(option) = args.next() {
        let value = args.next()?;
        match option.to_str()? {
            "--static" => statics = Some(PathBuf::from(value)),
            "--pre-tool-use" => pre_tool_use = Some(value.to_string_lossy().into_owned()),
            "--stop" => stop = Some(value.to_string_lossy().into_owned()),
            "--legacy" => legacy.push(value.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(settings::Options {
        statics: statics?,
        pre_tool_use,
        stop,
        legacy,
    })
}
