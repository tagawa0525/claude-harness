//! `claude-setup git-hooks --bin <claude-hooks>` が、~/.config/git/hooks に 4 つの git hook のファイル（claude-hooks の
//! git-hook を exec するだけの sh。ADR-0086）を置き、グローバルの core.hooksPath をそこに向けることを確かめる。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HOOKS: [&str; 4] = [
    "pre-commit",
    "pre-merge-commit",
    "commit-msg",
    "post-rewrite",
];

impl Home {
    fn new() -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "git-hooks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }

    fn gitconfig(&self) -> PathBuf {
        self.0.join(".gitconfig")
    }

    fn hooks(&self) -> PathBuf {
        self.0.join(".config/git/hooks")
    }

    fn git(&self) -> Command {
        let mut cmd = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("GIT_CONFIG_GLOBAL", self.gitconfig())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0);
        cmd
    }

    fn hooks_path(&self) -> Option<String> {
        let out = self
            .git()
            .args(["config", "--global", "--get", "core.hooksPath"])
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn run(&self, bin: &str) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.args(["git-hooks", "--bin", bin])
            .env("GIT_CONFIG_GLOBAL", self.gitconfig())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .output()
            .unwrap()
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn the_four_hook_files_exec_claude_hooks_and_hooks_path_points_at_them() {
    let home = Home::new();
    let out = home.run("/opt/claude hooks/bin/claude-hooks");
    assert!(out.status.success(), "{}", said(&out));
    for name in HOOKS {
        let text = std::fs::read_to_string(home.hooks().join(name)).unwrap();
        assert_eq!(
            text,
            format!(
                "#!/bin/sh\nexec '/opt/claude hooks/bin/claude-hooks' git-hook {name} \"$@\"\n"
            ),
            "{name}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.hooks().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "{name} は実行できる");
        }
    }
    let path = home.hooks_path().expect("core.hooksPath が入る");
    assert_eq!(
        std::fs::canonicalize(path).unwrap(),
        std::fs::canonicalize(home.hooks()).unwrap()
    );
}

#[test]
fn windows_paths_are_written_with_forward_slashes() {
    // sh は `\` を消すので、Windows のパス（cargo install の置き場）は `/` 区切りで書く
    let home = Home::new();
    let out = home.run(r"C:\Users\it's me\.cargo\bin\claude-hooks.exe");
    assert!(out.status.success(), "{}", said(&out));
    let text = std::fs::read_to_string(home.hooks().join("pre-commit")).unwrap();
    assert_eq!(
        text,
        "#!/bin/sh\nexec 'C:/Users/it'\\''s me/.cargo/bin/claude-hooks.exe' git-hook pre-commit \"$@\"\n"
    );
}

#[test]
fn running_twice_gives_the_same_files() {
    let home = Home::new();
    assert!(home.run("/opt/bin/claude-hooks").status.success());
    let first = std::fs::read_to_string(home.hooks().join("commit-msg")).unwrap();
    let again = home.run("/opt/bin/claude-hooks");
    assert!(again.status.success(), "{}", said(&again));
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("commit-msg")).unwrap(),
        first
    );
}

#[test]
fn another_global_hooks_path_is_not_overwritten() {
    // 利用者がほかの hook の置き場を使っていれば、黙って奪わずに止める
    let home = Home::new();
    let status = home
        .git()
        .args(["config", "--global", "core.hooksPath", "/opt/team-hooks"])
        .status()
        .unwrap();
    assert!(status.success());
    let out = home.run("/opt/bin/claude-hooks");
    assert_eq!(out.status.code(), Some(1));
    assert!(said(&out).contains("/opt/team-hooks"), "{}", said(&out));
    assert_eq!(home.hooks_path().as_deref(), Some("/opt/team-hooks"));
    assert!(!home.hooks().join("pre-commit").exists());
}

#[test]
fn a_missing_bin_is_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_claude-setup"))
        .arg("git-hooks")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
