//! `claude-hooks script git-info` が、今の Git の状態（ブランチ、未コミットの変更、未プッシュのコミット、stash、
//! worktree、関連の PR）とマージ済みのブランチの worktree を出すことを確かめる（ADR-0087。`~/.claude/scripts/git-info.sh`
//! が呼ぶ）。場合分けは、sh から呼ぶ `.claude/tests/scripts/git-workflow.sh` も確かめる

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
            "git-info-{}-{}",
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

    fn run(&self) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "git-info"])
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
fn shows_the_state_of_a_repository_without_a_remote() {
    let repo = Repo::new();
    std::fs::write(repo.app().join("b.txt"), "b\n").unwrap();
    let out = repo.run();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.starts_with("📍 現在のブランチ: main\n   上流: (未設定)\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("📝 未コミット変更:\n   ?? b.txt\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("📤 未プッシュコミット:\n   (上流ブランチ未設定)\n"),
        "{stdout}"
    );
    assert!(stdout.contains("📦 stash:\n   (なし)\n"), "{stdout}");
    assert!(
        stdout.contains("🔗 関連PR:\n   (GitHub リモートなし)\n"),
        "{stdout}"
    );
}

#[test]
fn a_worktree_of_a_merged_branch_is_reported() {
    let repo = Repo::new();
    let wt = repo.root.join("app-feat-done");
    repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feat/done",
        wt.to_str().unwrap(),
    ]);
    let out = repo.run();
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("⚠️ マージ済みブランチのworktreeがあります:"),
        "{stdout}"
    );
    assert!(stdout.contains("(feat/done) - マージ済み\n"), "{stdout}");
    assert!(
        stdout.ends_with("  /git-worktree feat/done --remove\n"),
        "{stdout}"
    );
}
