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

    /// claude-hooks の代わりのファイル（--bin はあるファイルを指さなければ止まる）
    fn bin(&self, name: &str) -> String {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        path.display().to_string()
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
    let bin = home.bin("claude hooks/bin/claude-hooks");
    let out = home.run(&bin);
    assert!(out.status.success(), "{}", said(&out));
    for name in HOOKS {
        let text = std::fs::read_to_string(home.hooks().join(name)).unwrap();
        assert_eq!(
            text,
            format!(
                "#!/bin/sh\nexec '{}' git-hook {name} \"$@\"\n",
                bin.replace('\\', "/")
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
fn quotes_in_the_path_are_kept_for_sh() {
    // sh は `\` を消すので `/` 区切りで書き、' は sh の引用の中で書き直す
    let home = Home::new();
    let bin = home.bin("it's me/claude-hooks");
    let out = home.run(&bin);
    assert!(out.status.success(), "{}", said(&out));
    let text = std::fs::read_to_string(home.hooks().join("pre-commit")).unwrap();
    let quoted = bin.replace('\\', "/").replace('\'', "'\\''");
    assert_eq!(
        text,
        format!("#!/bin/sh\nexec '{quoted}' git-hook pre-commit \"$@\"\n")
    );
}

#[test]
fn running_twice_gives_the_same_files() {
    let home = Home::new();
    let bin = home.bin("bin/claude-hooks");
    assert!(home.run(&bin).status.success());
    let first = std::fs::read_to_string(home.hooks().join("commit-msg")).unwrap();
    let again = home.run(&bin);
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
    let out = home.run(&home.bin("bin/claude-hooks"));
    assert_eq!(out.status.code(), Some(1));
    assert!(said(&out).contains("/opt/team-hooks"), "{}", said(&out));
    assert_eq!(home.hooks_path().as_deref(), Some("/opt/team-hooks"));
    assert!(!home.hooks().join("pre-commit").exists());
}

#[test]
fn a_hooks_path_from_an_included_file_counts() {
    // ~/.gitconfig から include した設定の core.hooksPath も、git が使うので奪わない
    let home = Home::new();
    let inc = home.0.join("team.inc");
    std::fs::write(&inc, "[core]\n\thooksPath = /opt/team-hooks\n").unwrap();
    std::fs::write(
        home.gitconfig(),
        format!(
            "[include]\n\tpath = {}\n",
            inc.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let out = home.run(&home.bin("bin/claude-hooks"));
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("/opt/team-hooks"), "{}", said(&out));
}

#[test]
fn hook_files_that_are_not_ours_are_not_replaced() {
    // 同じ置き場の、ハーネスのものでない hook のファイルは、黙って上書きせずに止める
    let home = Home::new();
    std::fs::create_dir_all(home.hooks()).unwrap();
    std::fs::write(home.hooks().join("pre-commit"), "#!/bin/sh\nrun-my-lint\n").unwrap();
    let out = home.run(&home.bin("bin/claude-hooks"));
    assert_eq!(out.status.code(), Some(1));
    assert!(said(&out).contains("pre-commit"), "{}", said(&out));
    assert_eq!(
        std::fs::read_to_string(home.hooks().join("pre-commit")).unwrap(),
        "#!/bin/sh\nrun-my-lint\n"
    );
}

#[test]
fn a_bin_that_does_not_exist_is_refused() {
    // 無いパスを書くと、その機械のすべてのコミットが止まる
    let home = Home::new();
    let out = home.run(&home.0.join("no/such/claude-hooks").display().to_string());
    assert_eq!(out.status.code(), Some(1));
    assert!(!home.hooks().join("pre-commit").exists());
    assert_eq!(home.hooks_path(), None);
}

#[test]
fn wrong_arguments_are_usage_errors() {
    for args in [
        vec!["git-hooks", "--bin"],
        vec!["git-hooks", "--pre-tool-use", "x"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_claude-setup"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn a_relative_bin_is_refused() {
    // git は hook を各リポジトリの直下で起動するので、相対のパスはほかのリポジトリで見つからない
    let home = Home::new();
    home.bin("rel/claude-hooks");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .args(["git-hooks", "--bin", "rel/claude-hooks"])
        .current_dir(&home.0)
        .env("GIT_CONFIG_GLOBAL", home.gitconfig())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", &home.0)
        .env("USERPROFILE", &home.0)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(!home.hooks().join("pre-commit").exists());
}

#[test]
fn an_unreadable_hook_file_counts_as_not_ours() {
    // UTF-8 でない（コンパイルしたものや別の符号化の）hook も、利用者のものとして上書きしない
    let home = Home::new();
    std::fs::create_dir_all(home.hooks()).unwrap();
    std::fs::write(home.hooks().join("commit-msg"), b"\x7fELF\xff\xfe").unwrap();
    let out = home.run(&home.bin("bin/claude-hooks"));
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert_eq!(
        std::fs::read(home.hooks().join("commit-msg")).unwrap(),
        b"\x7fELF\xff\xfe"
    );
}

#[test]
fn a_system_hooks_path_is_not_taken_over() {
    // system の core.hooksPath も git は使うので、global に書いて上書きしない
    let home = Home::new();
    let system = home.0.join("system.gitconfig");
    std::fs::write(&system, "[core]\n\thooksPath = /opt/corp-hooks\n").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .args(["git-hooks", "--bin", &home.bin("bin/claude-hooks")])
        .env("GIT_CONFIG_GLOBAL", home.gitconfig())
        .env("GIT_CONFIG_SYSTEM", &system)
        .env("HOME", &home.0)
        .env("USERPROFILE", &home.0)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(said(&out).contains("/opt/corp-hooks"), "{}", said(&out));
    assert_eq!(home.hooks_path(), None);
}
