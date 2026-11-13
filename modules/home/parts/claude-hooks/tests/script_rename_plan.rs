//! `claude-hooks script rename-plan --list | <branch-name> [--file <path>]` が、docs/plans/ のランダムな名前の計画書を
//! 連番つきの名前にすることを確かめる（ADR-0087。`~/.claude/scripts/rename-plan.sh` が呼ぶ）。場合分けは、sh から呼ぶ
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
            "rename-plan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs/plans")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write("docs/plans/001_first.md", "first\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "docs: first"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&self.root)
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
        std::fs::write(self.root.join(path), text).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "rename-plan"])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_tracked_random_plan_gets_the_next_number_and_is_committed() {
    let repo = Repo::new();
    repo.write("docs/plans/happy-blue-fox.md", "plan\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "docs: plan"]);
    let out = repo.run(&["feat/login-page"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "RENAMED: docs/plans/happy-blue-fox.md -> docs/plans/002_login_page.md\nCOMMITTED: yes\n"
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "docs: rename plan happy-blue-fox to 002_login_page.md"
    );
}

#[test]
fn an_untracked_plan_is_only_moved_and_list_shows_the_random_ones() {
    let repo = Repo::new();
    repo.write("docs/plans/quiet-red-owl.md", "plan\n");
    let out = repo.run(&["--list"]);
    assert_eq!(text(&out.stdout), "docs/plans/quiet-red-owl.md\n");
    let out = repo.run(&["fix/a/b-c"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "RENAMED: docs/plans/quiet-red-owl.md -> docs/plans/002_a_b_c.md\nCOMMITTED: no\n"
    );
}

#[test]
fn several_random_plans_need_file_and_bad_arguments_are_errors() {
    let repo = Repo::new();
    repo.write("docs/plans/a-b-c.md", "a\n");
    repo.write("docs/plans/d-e-f.md", "d\n");
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("--file で対象を指定"),
        "{}",
        text(&out.stderr)
    );
    for args in [
        &[][..],
        &["a", "b"][..],
        &["a", "--file"][..],
        &["a", "--force"][..],
    ] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: rename-plan.sh"),
            "{args:?}"
        );
    }
}

#[test]
fn hidden_files_are_not_plans_and_symlinked_plans_are() {
    let repo = Repo::new();
    repo.write("docs/plans/.hidden.md", "h\n");
    let elsewhere = repo.root.join("elsewhere.md");
    std::fs::write(&elsewhere, "e\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, repo.root.join("docs/plans/link-plan.md")).unwrap();
    let out = repo.run(&["--list"]);
    #[cfg(unix)]
    assert_eq!(text(&out.stdout), "docs/plans/link-plan.md\n");
    #[cfg(not(unix))]
    assert_eq!(text(&out.stdout), "");
}
