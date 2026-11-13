//! git hook の結合テストが、hook の呼ぶ外のコマンド（ja-terms、commit-refs、gitleaks、書式と lint のツールなど）の
//! 代わりに置く偽のコマンド。Windows ではスクリプトを直に起動できないので、実行ファイルにする（`cargo test` が
//! examples もビルドする）。同じ偽のコマンドを別の名前でコピーして置けるよう、`FAKE_<名前>_<鍵>` があればそれを
//! `FAKE_TOOL_<鍵>` より先に読む（名前は実行ファイルの名前を大文字にし、`-` を `_` にしたもの。`FAKE_NIXFMT_EXIT` など）。
//!
//! - `LOG`: 渡された引数を、空白でつないで 1 行ずつ足すファイル（名前の付いた偽のコマンドは、行の先頭に実行ファイルの名前を付ける）
//! - `STDIN`: 標準入力をそのまま書くファイル（あれば足す）
//! - `ECHO`: 値があれば、標準入力をそのまま標準出力に書く
//! - `OUT`: 標準出力に書く文字列
//! - `ERR`: 標準エラーに書く文字列
//! - `CWD`: 起動された作業ディレクトリを 1 行ずつ足すファイル
//! - `PARENT_MODE`: 作業ディレクトリの親の権限（Unix の 8 進。Windows は何も書かない）を 1 行ずつ足すファイル
//! - `EXIT`: 終了コード（既定は 0）

use std::io::{Read, Write};

fn append(path: Option<std::ffi::OsString>, what: &str, text: &[u8]) {
    if let Some(path) = path {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|e| panic!("{what} を開けない: {e}"));
        file.write_all(text)
            .unwrap_or_else(|e| panic!("{what} に書けない: {e}"));
    }
}

fn main() {
    let exe = std::env::current_exe().expect("実行ファイルのパスを読める");
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = stem.to_uppercase().replace('-', "_");
    let named = name != "FAKE_TOOL";
    let var = |key: &str| {
        let own = format!("FAKE_{name}_{key}");
        std::env::var_os(&own).or_else(|| std::env::var_os(format!("FAKE_TOOL_{key}")))
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = if named {
        format!("{stem} {}\n", args.join(" "))
    } else {
        format!("{}\n", args.join(" "))
    };
    append(var("LOG"), "LOG", line.as_bytes());
    if var("CWD").is_some() {
        let cwd = std::env::current_dir().expect("作業ディレクトリを読める");
        append(var("CWD"), "CWD", format!("{}\n", cwd.display()).as_bytes());
    }
    #[cfg(unix)]
    if var("PARENT_MODE").is_some() {
        use std::os::unix::fs::PermissionsExt;
        let cwd = std::env::current_dir().expect("作業ディレクトリを読める");
        let parent = cwd.parent().expect("親がある");
        let mode = std::fs::metadata(parent)
            .expect("親を読める")
            .permissions()
            .mode()
            & 0o777;
        append(
            var("PARENT_MODE"),
            "PARENT_MODE",
            format!("{mode:o}\n").as_bytes(),
        );
    }
    let mut input = Vec::new();
    if var("STDIN").is_some() || var("ECHO").is_some() {
        std::io::stdin()
            .read_to_end(&mut input)
            .expect("標準入力を読める");
    }
    append(var("STDIN"), "STDIN", &input);
    if var("ECHO").is_some() {
        std::io::stdout()
            .write_all(&input)
            .expect("標準出力に書ける");
    }
    if let Some(out) = var("OUT") {
        print!("{}", out.to_string_lossy());
    }
    if let Some(err) = var("ERR") {
        eprint!("{}", err.to_string_lossy());
    }
    let code = var("EXIT")
        .and_then(|c| c.to_string_lossy().parse().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
