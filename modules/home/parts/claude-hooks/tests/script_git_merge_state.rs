//! `claude-hooks script git-merge-state [branch]` が、/git-merge の事前の確認（対象の検証と、マージの前に要る状態）を
//! 出すことを確かめる（ADR-0087。`~/.claude/scripts/git-merge-state.sh` が呼ぶ）。積んだブランチ、worktree、
//! PROPOSED_ADRS の Refs: の読み方などの場合分けは、sh から呼ぶ `.claude/tests/scripts/git-merge-state.sh` も確かめる

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
            "git-merge-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write("docs/adr/0001-a.md", "---\nstatus: proposed\n---\n\n# A\n");
        repo.commit("docs(adr): add a");
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
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "git-merge-state"])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("no-global-config"))
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn reports_the_state_before_the_merge() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a\n\nRefs: #1, ADR-0001");
    let root = repo.git(&["rev-parse", "--show-toplevel"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!(
            "TARGET: feat/x\nDEFAULT: main\nCOMMITS: 1\nBEHIND: 0\nSTACKED_ON: none\n\
             TARGET_WORKTREE: {}\nDEFAULT_WORKTREE: none\nPROPOSED_ADRS: ADR-0001\n",
            root.trim()
        )
    );
}

#[test]
fn the_default_branch_or_a_missing_branch_is_not_a_target() {
    let repo = Repo::new();
    let out = repo.run(&["main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("既定ブランチ main 自身"));
    let out = repo.run(&["no-such"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("ローカルにブランチ no-such がありません"));
}

#[test]
fn uncommitted_changes_stop_the_merge() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("feat: a");
    repo.write("b.txt", "b\n");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("未コミットの変更があります"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn a_github_remote_belongs_to_the_pr_flow() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/octo/repo.git",
    ]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("GitHub リモートがあります"));
}
