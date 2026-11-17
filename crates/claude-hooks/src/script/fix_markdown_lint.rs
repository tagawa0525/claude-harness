//! `fix-markdown-lint.sh <file>...`: markdownlint --fix の直せない Markdown の違反を直す（中身は fix-markdown-lint の
//! crate。直す規則はその冒頭）。
//!
//! 直したファイルごとに `Fixed: <file>` を出す。直すものが無ければ書かず `OK: <file>` を出す。無いファイルは黙って
//! 飛ばし、`--` で始まる引数はファイルに数えない。UTF-8 で読めないファイルでは、そこで止まって 1 で終える。

use fix_markdown_lint::fix_file;
use std::path::Path;

const USAGE: &str = "Usage: fix-markdown-lint.sh <file>...

Fixes:
    MD040  Fenced code blocks should have a language specified (via heuristic)
    MD060  Table column alignment (CJK full-width aware)";

pub fn main(args: &[String]) -> i32 {
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.is_empty() {
        println!("{USAGE}");
        return 1;
    }
    for file in files {
        match fix_file(Path::new(file)) {
            Ok(Some(true)) => println!("Fixed: {file}"),
            Ok(Some(false)) => println!("OK: {file}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("ERROR: {file}: {e}");
                return 1;
            }
        }
    }
    0
}
