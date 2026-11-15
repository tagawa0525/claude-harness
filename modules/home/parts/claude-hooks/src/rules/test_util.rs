//! ルールの単体テストが共有する部品（一時のディレクトリ、git のリポジトリ、偽のコマンドの起動待ち）

// 使うルールのテストが Unix だけのもの（sh の偽のコマンド）があるので、Windows では使われない部品がある
#![cfg_attr(not(unix), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::process::Command;

/// テストの間だけある一時のディレクトリ（テストが panic しても消す）。テストは並行に走るので、同じ tag でも
/// 呼ぶたびに別のディレクトリにする
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "claude-hooks-test-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// テストのグローバルの core.hooksPath。`git_init` のリポジトリはこれをローカルに持つ（手元とサンドボックスで
/// グローバルの設定が違っても、グローバルの hook を使うリポジトリにする）
pub const GLOBAL_HOOKS: &str = "/nonexistent/global-hooks";

pub fn global_hooks(_: &Path) -> Option<String> {
    Some(GLOBAL_HOOKS.to_string())
}

pub fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

pub fn git_init(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "core.hooksPath", GLOBAL_HOOKS]);
}

/// sh の偽のコマンドを書いて実行の権限を付け、起動できるまで待つ。並行するテストが子を fork した直後は、書き込みに
/// 開いた fd が exec までその子に残り、実行が ETXTBSY（Text file busy）で失敗する。`probe_args` は、偽のコマンドが
/// 何もせず終わる引数（偽の検査は引数が合わないと 3 で終わる）
#[cfg(unix)]
pub fn write_fake_command(path: &Path, content: &str, probe_args: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..100 {
        match Command::new(path).args(probe_args).output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => return,
        }
    }
    panic!(
        "偽のコマンドが 1 秒たっても Text file busy のまま: {}",
        path.display()
    );
}
