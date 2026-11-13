//! `claude-hooks script post-merge-cleanup <branch>` が、マージの後にブランチと worktree を片付けることを確かめる
//! （ADR-0087。`~/.claude/scripts/post-merge-cleanup.sh` が呼ぶ）。痕跡、fetch と pull、NOTE、予約のタグ、リモートの
//! ブランチの場合分けは、sh から呼ぶ `.claude/tests/scripts/git-workflow.sh` も確かめる

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
            "post-merge-cleanup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write("a.txt", "a\n");
        repo.commit("init");
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

    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.app().join(path), text).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "post-merge-cleanup"])
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
fn a_merged_branch_and_its_worktree_are_removed() {
    let repo = Repo::new();
    let wt = repo.root.join("app-feat-x");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/x",
        wt.to_str().unwrap(),
    ]);
    repo.git(&["merge", "-q", "--ff-only", "feat/x"]);
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("WORKTREE_REMOVED: "), "{stdout}");
    assert!(
        stdout
            .ends_with("RESERVATIONS_RELEASED: none\nLOCAL_BRANCH: deleted\nREMOTE_BRANCH: none\n"),
        "{stdout}"
    );
    assert!(!wt.exists());
    assert_eq!(repo.git(&["branch", "--list", "feat/x"]), "");
}

#[test]
fn an_unmerged_branch_is_left_as_it_is() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/y"]);
    repo.write("b.txt", "b\n");
    repo.commit("feat: b");
    repo.git(&["switch", "-q", "main"]);
    let out = repo.run(&["feat/y"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("feat/y は main にまだマージされていません"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(repo.git(&["branch", "--list", "feat/y"]), "feat/y");
}

#[test]
fn a_branch_without_any_trace_changes_nothing() {
    let repo = Repo::new();
    let out = repo.run(&["feat/none"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("feat/none の痕跡"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn bad_arguments_and_the_default_branch_are_errors() {
    let repo = Repo::new();
    for args in [&[][..], &["a", "b"][..], &[""][..]] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: post-merge-cleanup.sh"),
            "{args:?}"
        );
    }
    let out = repo.run(&["main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("既定ブランチ main は削除しません"));
}
