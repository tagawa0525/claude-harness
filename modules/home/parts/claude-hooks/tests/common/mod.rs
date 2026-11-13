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

/// 偽のコマンド（[`fake_tool`]）を dest に置く。ハードリンクにするのは、コピーすると書き込みに開いた fd が、並行するテストが
/// その間に fork した子に exec まで残り、置いたコマンドの起動が ETXTBSY（Text file busy）で失敗しうるため（リンクは
/// 書き込みに開かない）。同じファイルシステムに無くリンクできなければコピーし、起動できるまで待つ
#[allow(dead_code)]
pub fn place_fake_tool(dest: &Path) {
    let _ = std::fs::remove_file(dest);
    if std::fs::hard_link(fake_tool(), dest).is_ok() {
        return;
    }
    std::fs::copy(fake_tool(), dest).unwrap();
    executable(dest);
    for _ in 0..100 {
        match Command::new(dest).output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => return,
        }
    }
    panic!(
        "偽のコマンドが 1 秒たっても Text file busy のまま: {}",
        dest.display()
    );
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
