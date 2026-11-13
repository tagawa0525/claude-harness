//! claude-setup: ハーネスを Claude Code の設定に配る設置のコマンド（ADR-0084）
//!
//! 使い方:
//!
//! - `claude-setup sync [<ソースのリポジトリ>]`: ハーネスの .claude を ~/.claude に同期する（[`sync`]）
//!
//! 終了コード: 0 = 済んだ、1 = 止めた（理由は標準エラー）、2 = 使い方の誤り

mod sync;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage: claude-setup sync [source-repo]";

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if matches!(args.as_slice(), [c, h] if c == "sync" && (h == "--help" || h == "-h"))
        || matches!(args.as_slice(), [h] if h == "--help" || h == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let source = match args.as_slice() {
        [command] if command == "sync" => None,
        [command, source] if command == "sync" => Some(Path::new(source)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(home) = sync::home() else {
        eprintln!("❌ ホームのディレクトリが分かりません（HOME、Windows では USERPROFILE）");
        return ExitCode::from(1);
    };
    match sync::run(&home, source) {
        Ok(()) => ExitCode::SUCCESS,
        Err(sync::Stop(lines)) => {
            for line in lines {
                eprintln!("{line}");
            }
            ExitCode::from(1)
        }
    }
}
