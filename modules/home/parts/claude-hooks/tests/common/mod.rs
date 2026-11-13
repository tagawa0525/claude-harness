//! 結合テストの共通の部品（git hook と git の起動、`claude-hooks script` の偽の gh）

#[allow(dead_code)]
pub mod fake_gh;

use std::path::{Path, PathBuf};
use std::process::Command;

/// examples/fake_tool の実行ファイル（テストの実行ファイルは target/<profile>/deps にある）。`cargo test` は examples も
/// ビルドする（`--test` で絞ると作らないので、先に `cargo build --examples` を流す）
#[allow(dead_code)]
pub fn fake_tool() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    profile
        .join("examples")
        .join(format!("fake_tool{}", std::env::consts::EXE_SUFFIX))
}

/// sh に書くパス（Windows の `\` は sh の中で消えるので `/` にする）
#[allow(dead_code)]
pub fn sh_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// 実行の権限を付ける（Windows には無い）
#[allow(dead_code)]
pub fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// 呼んだ側のリポジトリ（run-checks.sh を実行する git の hook の中の GIT_DIR、GIT_INDEX_FILE など）を見せない。
/// 残すと、テストの git が外のリポジトリに当たる
#[allow(dead_code)]
pub fn forget_git_env(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd
}
