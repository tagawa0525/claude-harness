//! claude-hooks: Claude Code の PreToolUse hook。
//!
//! 標準入力の hook JSON（tool_name / tool_input.command / tool_input.run_in_background / tool_input.prompt / cwd）を
//! 読み、tool_name のツールを検査するルールを評価して、deny / additionalContext を JSON で返す。Bash のルールは
//! コマンドを構文解析して判定し、Agent のルールは prompt を判定する。
//! 判定できなかった理由（git を起動できない、ルールの panic）は systemMessage で利用者に見せる（ADR-0033）。
//! 出力なし = 許可。終了コードは常に 0（hook の不具合でツール呼び出しを壊さない）。
//!
//! Usage: claude-hooks pre-tool-use [--rule <name>]...
//!   --rule を指定するとそのルールだけを評価する（テストが 1 ルールずつ検証するため）
//!        claude-hooks local-review-covers <owner>/<repo> <PR 番号> <head の SHA>
//!   head がローカルレビューの記録で覆われているか（記録の後に rebase だけした head を含む）を pre-merge-check と同じ
//!   判定で確かめる。rebase だけかは cwd の clone で比べる。覆われていれば終了コード 0、いなければ 1、引数の数の誤りは 2、
//!   記録を取得できないか、引数の形（完全な 16 進の SHA、数の PR 番号）が違えば 3（decide-next.sh と request-rereview.sh が使う）

mod gh;
mod git;
mod git_args;
mod input;
mod output;
mod rules;
mod shell;
mod unwrap;

use std::io::Read;

// .claude/tests/hooks.sh と modules/home/parts/tests/main-commit-gates.sh は、引数なしで呼んで `rules: `
// で始まる行が出ることで、渡されたバイナリが起動できるかを確かめる
fn usage() -> ! {
    eprintln!("Usage: claude-hooks pre-tool-use [--rule <name>]...");
    eprintln!("       claude-hooks local-review-covers <owner>/<repo> <PR 番号> <head の SHA>");
    eprintln!("rules: {}", rules::names().join(", "));
    std::process::exit(2);
}

fn local_review_covers(args: &[String]) -> ! {
    let [repo, number, head] = args else {
        usage();
    };
    let covered = std::env::current_dir()
        .map_err(|_| ())
        .and_then(|dir| rules::local_review_covers_for_pr(&dir, repo, number, head));
    // gh や git を起動できなかった理由（output::notice）は、覆われていないときの終了コード 1 と見分けられるよう標準エラーに出す
    for notice in output::take_notices() {
        eprintln!("{notice}");
    }
    std::process::exit(match covered {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(()) => {
            eprintln!(
                "claude-hooks: PR のローカルレビューの記録を取得できません（引数の形か gh の失敗）"
            );
            3
        }
    });
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        Some("pre-tool-use") => {}
        Some("local-review-covers") => local_review_covers(&args[1..]),
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
    let input = std::io::stdin()
        .read_to_string(&mut raw)
        .ok()
        .and_then(|_| input::Input::parse(&raw));
    let Some(input) = input else {
        output::notice(
            "claude-hooks: hook の入力を読めません。このツールの呼び出しを判定していません"
                .to_string(),
        );
        if let Some(json) = output::render(&[], &output::take_notices()) {
            println!("{json}");
        }
        return;
    };
    let rules = rules::for_tool(&input.tool_name);
    if rules.is_empty() {
        return;
    }

    // panic は「判定できない」であって「ツールを止める」ではない。ルールごとに捕捉し、ほかのルールの判定（deny）を
    // 残す。panic の文は既定の panic hook が標準エラーにも出す（テストが panic を見分けるため）
    let mut findings = Vec::new();
    match std::panic::catch_unwind(|| shell::Shell::parse(&input.command)) {
        Ok(shell) => {
            for rule in &rules {
                if selected.is_empty() || selected.contains(&rule.name()) {
                    // input と shell はルールが書き換えないので、panic の後もほかのルールに渡してよい。panic したルールが
                    // それまでに足した理由（output::notice）は残る
                    let run = std::panic::AssertUnwindSafe(|| rules::run(*rule, &input, &shell));
                    match std::panic::catch_unwind(run) {
                        Ok(f) => findings.extend(f),
                        Err(payload) => output::notice(format!(
                            "claude-hooks: ルール {} が panic で止まり、このツールの呼び出しを判定していません: {}",
                            rule.name(),
                            panic_message(&*payload)
                        )),
                    }
                }
            }
        }
        Err(payload) => output::notice(format!(
            "claude-hooks: コマンドの解析が panic で止まり、このコマンドを判定していません: {}",
            panic_message(&*payload)
        )),
    }
    if let Some(json) = output::render(&findings, &output::take_notices()) {
        println!("{json}");
    }
}

/// panic の文（panic! の引数が文字列でなければ、その旨）
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("（文字列でない panic）")
}
