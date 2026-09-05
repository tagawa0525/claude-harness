//! claude-hooks: Claude Code の PreToolUse hook。
//!
//! 標準入力の hook JSON（tool_name / tool_input.command / tool_input.run_in_background / cwd）を読み、
//! Bash ツールのコマンドを構文解析して各ルールを評価し、deny / additionalContext を JSON で返す。
//! 出力なし = 許可。終了コードは常に 0（hook の不具合でツール呼び出しを壊さない）。
//!
//! Usage: claude-hooks pre-tool-use [--rule <name>]...
//!   --rule を指定するとそのルールだけを評価する（テストが 1 ルールずつ検証するため）

mod gh;
mod git;
mod input;
mod output;
mod rules;
mod shell;

use std::io::Read;

fn usage() -> ! {
    eprintln!("Usage: claude-hooks pre-tool-use [--rule <name>]...");
    eprintln!("rules: {}", rules::names().join(", "));
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        Some("pre-tool-use") => {}
        _ => usage(),
    }
    let mut selected: Vec<&str> = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rule" => match it.next() {
                Some(name) if rules::by_name(name).is_some() => selected.push(name),
                Some(name) => {
                    eprintln!("unknown rule: {name}");
                    usage();
                }
                None => usage(),
            },
            _ => usage(),
        }
    }

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return;
    }
    let Some(input) = input::Input::parse(&raw) else {
        return;
    };
    if input.tool_name != "Bash" {
        return;
    }

    // ルール側の panic は「判定できない」であって「ツールを止める」ではない
    let result = std::panic::catch_unwind(|| {
        let shell = shell::Shell::parse(&input.command);
        let mut findings = Vec::new();
        for rule in rules::all() {
            if selected.is_empty() || selected.contains(&rule.name()) {
                findings.extend(rule.check(&input, &shell));
            }
        }
        output::render(&findings)
    });
    if let Ok(Some(json)) = result {
        println!("{json}");
    }
}
