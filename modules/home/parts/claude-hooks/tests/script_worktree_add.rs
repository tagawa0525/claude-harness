//! `claude-hooks script worktree-add <branch> [--from <ref>] [--carry-changes]` が、命名の規則に従って worktree を作る
//! ことを確かめる（ADR-0087。`~/.claude/scripts/worktree-add.sh` が呼ぶ）。fetch、--carry-changes、.worktreelink の
//! 場合分けは、sh から呼ぶ `.claude/tests/scripts/git-workflow.sh` も確かめる

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Repo {
    /// 一時のディレクトリ。メインの worktree は `<root>/app`、作る worktree は `<root>/app-<branch>`
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
            "worktree-add-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app")).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write(".gitignore", "/shared\n");
        repo.commit("init");
        repo
    }

    fn app(&self) -> PathBuf {
        self.root.join("app")
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
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

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.app(), args)
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.app().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "worktree-add"])
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
fn a_new_branch_is_made_from_the_default_branch_next_to_the_main_worktree() {
    let repo = Repo::new();
    let out = repo.run(&["feat/login"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let dir = repo.root.join("app-feat-login");
    assert_eq!(
        text(&out.stdout),
        format!(
            "WORKTREE: {}\nBRANCH: feat/login\nCREATED_BRANCH: yes\nBASE: main\n",
            dir.display()
        )
    );
    assert_eq!(
        repo.git_in(&dir, &["branch", "--show-current"]),
        "feat/login"
    );
    // 起点の追跡ブランチを上流にしない（上流が main だと push 先が main になる）
    let upstream = forget_git_env(&mut Command::new("git"))
        .args(["config", "--get", "branch.feat/login.merge"])
        .current_dir(repo.app())
        .env("GIT_CONFIG_GLOBAL", repo.root.join("no-global-config"))
        .output()
        .unwrap();
    assert_eq!(text(&upstream.stdout), "");
}

#[test]
fn an_existing_branch_is_checked_out_and_from_is_refused_for_it() {
    let repo = Repo::new();
    repo.git(&["branch", "fix/typo"]);
    let out = repo.run(&["fix/typo", "--from", "main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--from は新しいブランチにだけ"));
    let out = repo.run(&["fix/typo"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("CREATED_BRANCH: no\n"));
    assert!(!text(&out.stdout).contains("BASE:"));
}

#[test]
fn worktreelink_shares_ignored_paths_of_the_main_worktree() {
    let repo = Repo::new();
    std::fs::create_dir_all(repo.app().join("shared")).unwrap();
    repo.write(".worktreelink", "# 共有する\nshared\n");
    repo.commit("add worktreelink");
    let out = repo.run(&["feat/x"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).ends_with("SHARED: shared\n"),
        "{}",
        text(&out.stdout)
    );
    let link = repo.root.join("app-feat-x/shared");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        repo.app().join("shared")
    );
}

#[test]
fn bad_arguments_or_an_existing_directory_are_errors() {
    let repo = Repo::new();
    for args in [
        &[][..],
        &["a", "b"][..],
        &["a", "--from"][..],
        &["a", "--force"][..],
    ] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: worktree-add.sh"),
            "{args:?}"
        );
    }
    std::fs::create_dir_all(repo.root.join("app-feat-y")).unwrap();
    let out = repo.run(&["feat/y"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("作成先が既に存在します"));
}
