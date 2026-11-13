//! git hook の結合テストが、hook の呼ぶ外のコマンド（ja-terms、commit-refs など）の代わりに置く偽のコマンド。Windows
//! ではスクリプトを直に起動できないので、実行ファイルにする（`cargo test` が examples もビルドする）。
//!
//! - `FAKE_TOOL_LOG`: 渡された引数を、空白でつないで 1 行ずつ足すファイル
//! - `FAKE_TOOL_STDIN`: 標準入力をそのまま書くファイル（あれば足す）
//! - `FAKE_TOOL_OUT`: 標準出力に書く文字列
//! - `FAKE_TOOL_ERR`: 標準エラーに書く文字列
//! - `FAKE_TOOL_CWD`: 起動された作業ディレクトリを 1 行ずつ足すファイル
//! - `FAKE_TOOL_EXIT`: 終了コード（既定は 0）

use std::io::{Read, Write};

fn append(var: &str, text: &[u8]) {
    if let Some(path) = std::env::var_os(var) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|e| panic!("{var} を開けない: {e}"));
        file.write_all(text)
            .unwrap_or_else(|e| panic!("{var} に書けない: {e}"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    append("FAKE_TOOL_LOG", format!("{}\n", args.join(" ")).as_bytes());
    let cwd = std::env::current_dir().expect("作業ディレクトリを読める");
    append("FAKE_TOOL_CWD", format!("{}\n", cwd.display()).as_bytes());
    if std::env::var_os("FAKE_TOOL_STDIN").is_some() {
        let mut input = Vec::new();
        std::io::stdin()
            .read_to_end(&mut input)
            .expect("標準入力を読める");
        append("FAKE_TOOL_STDIN", &input);
    }
    if let Ok(out) = std::env::var("FAKE_TOOL_OUT") {
        print!("{out}");
    }
    if let Ok(err) = std::env::var("FAKE_TOOL_ERR") {
        eprint!("{err}");
    }
    let code = std::env::var("FAKE_TOOL_EXIT")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
