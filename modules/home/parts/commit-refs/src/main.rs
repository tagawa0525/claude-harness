//! commit-refs: 履歴の書き換えで変わったコミットの番号を、文書の中で付け直す（ADR-0012）。

use std::process::ExitCode;

const USAGE: &str = "Usage: commit-refs remap [MAP]";

fn main() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
