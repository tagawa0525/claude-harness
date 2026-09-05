//! git の呼び出し。PATH の git を使う（テストは PATH を差し替えて挙動を作る）

use std::path::Path;
use std::process::Command;

/// `git <args>` を dir で実行し、成功なら標準出力（末尾改行を除く）を返す。
/// 失敗（非 0 終了、dir が無い、git が無い）は None
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// リモートのいずれかが github.com を指すか（PR フローの適用対象か）
pub fn has_github_remote(dir: &Path) -> bool {
    git(dir, &["remote", "-v"]).is_some_and(|s| s.contains("github.com"))
}

pub fn current_branch(dir: &Path) -> String {
    git(dir, &["branch", "--show-current"]).unwrap_or_default()
}
