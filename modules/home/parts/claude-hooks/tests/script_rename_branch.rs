//! `claude-hooks script rename-branch <new-name> [--remote]` が、今の feature ブランチの名前を変えることを確かめる
//! （ADR-0087。`~/.claude/scripts/rename-branch.sh` が呼ぶ）。リモートと open の PR の場合分けは、sh から呼ぶ
//! `.claude/tests/scripts/git-workflow.sh` も確かめる

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Repo {
    root: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Repo {
    fn new() -> Repo {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "rename-branch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.app().join("a.txt"), "a\n").unwrap();
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "init"]);
        repo
    }

    fn app(&self) -> PathBuf {
        self.root.join("app")
    }

    fn git(&self, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(self.app())
            // 利用者のグローバルの設定（グローバルの git hook など）を読まない
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "rename-branch"])
            .args(args)
            .current_dir(self.app())
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_local_branch_is_renamed() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/old"]);
    let out = repo.run(&["feat/new"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "RENAMED: feat/old -> feat/new\nREMOTE: none\n"
    );
    assert_eq!(repo.git(&["branch", "--show-current"]), "feat/new");
}

#[test]
fn a_branch_on_the_remote_stops_without_remote_and_is_renamed_there_with_it() {
    let repo = Repo::new();
    let bare = repo.root.join("origin.git");
    repo.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    repo.git(&["switch", "-q", "-c", "feat/old"]);
    repo.git(&["push", "-q", "-u", "origin", "feat/old"]);
    let out = repo.run(&["feat/new"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stdout).starts_with("STOP: "),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(repo.git(&["branch", "--show-current"]), "feat/old");
    let out = repo.run(&["feat/new", "--remote"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "RENAMED: feat/old -> feat/new\nREMOTE: updated\n"
    );
    assert_eq!(
        repo.git(&["ls-remote", "--heads", "origin"])
            .lines()
            .count(),
        1
    );
    assert!(
        repo.git(&["ls-remote", "--heads", "origin"])
            .ends_with("refs/heads/feat/new")
    );
}

#[test]
fn the_default_branch_the_same_name_and_bad_arguments_are_errors() {
    let repo = Repo::new();
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("main はリネームしません"));
    repo.git(&["switch", "-q", "-c", "feat/same"]);
    let out = repo.run(&["feat/same"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("現在のブランチ名と同じです"));
    for args in [&[][..], &["a", "b"][..], &["a", "--force"][..]] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: rename-branch.sh"),
            "{args:?}"
        );
    }
}
