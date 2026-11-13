//! 子のプロセスに標準入力で渡して、終わりを受ける。一時の場所を作る（git hook の中身が使う）

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

/// 持ち主だけが読める一時のディレクトリ（終わったら消す）
pub struct TempDir(pub std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 一時のディレクトリを作る。ステージした中身や HEAD の .gitignore を書き出すので、持ち主だけが読める場所にする
/// （mktemp -d と同じ 0700）。名前が既にあれば、ほかの人が作ったかもしれないので使わない（create_dir_all でなく
/// create）。作れた後にだけ片付けの対象にする（作れなかった場所は、ほかの人のものかもしれないので消さない）
pub fn private_temp_dir(prefix: &str) -> Result<TempDir, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&dir)
        .map_err(|e| format!("一時のディレクトリ {} を作れません（{e}）", dir.display()))?;
    Ok(TempDir(dir))
}
