//! git hook の結合テストが、hook の呼ぶ外のコマンド（ja-terms など）の代わりに置く偽のコマンド。Windows では
//! スクリプトを直に起動できないので、実行ファイルにする（`cargo test` が examples もビルドする）。
//!
//! - `FAKE_TOOL_LOG`: 渡された引数を、空白でつないで 1 行ずつ足すファイル
//! - `FAKE_TOOL_EXIT`: 終了コード（既定は 0）

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(log) = std::env::var_os("FAKE_TOOL_LOG") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .expect("FAKE_TOOL_LOG を開ける");
        writeln!(file, "{}", args.join(" ")).expect("FAKE_TOOL_LOG に書ける");
    }
    let code = std::env::var("FAKE_TOOL_EXIT")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
