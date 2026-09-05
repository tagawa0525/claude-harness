//! gh の呼び出し。PATH の gh を使う（テストは偽の gh を PATH 先頭に置き、
//! 引数列 "$*" の前方一致で応答するため、引数の順序と --jq フィルタは bash 版と揃える）

use std::path::Path;
use std::process::Command;

/// `gh <args>` を dir で実行し、成功なら標準出力を返す。失敗は Err
pub fn gh(dir: &Path, args: &[&str]) -> Result<String, ()> {
    let out = Command::new("gh")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|_| ())?;
    if !out.status.success() {
        return Err(());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string())
}
