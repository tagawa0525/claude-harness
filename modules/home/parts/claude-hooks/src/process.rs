//! 子のプロセスに標準入力で渡して、終わりを受ける（git hook の中身が使う）

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// 子が終わらなかった理由。起動できなかった（何もしていない）のと、起動した後に終わりを受けられなかった（途中まで
/// 動いたかもしれない）のとを分ける
pub enum Unfinished {
    NotStarted(std::io::Error),
    Lost(std::io::Error),
}

/// cmd に input を標準入力で渡して実行する（標準出力と標準エラーの行き先は呼ぶ側が決める）。入力を書き終える前に
/// 相手が標準出力を埋めても詰まらないよう、別のスレッドで書く。入力を読まずに終わる子もあるので、書けないこと
/// （EPIPE）は失敗にしない
pub fn output_with_input(cmd: &mut Command, input: &[u8]) -> Result<Output, Unfinished> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .spawn()
        .map_err(Unfinished::NotStarted)?;
    let stdin = child.stdin.take();
    std::thread::scope(|scope| {
        if let Some(mut stdin) = stdin {
            scope.spawn(move || {
                let _ = stdin.write_all(input);
            });
        }
        child.wait_with_output()
    })
    .map_err(Unfinished::Lost)
}
