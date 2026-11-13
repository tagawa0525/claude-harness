//! `claude-hooks script base-moved [branch] [--base BASE]` が、比較元が分岐点から進み、ブランチと同じパスを触って
//! いれば知らせることを確かめる（ADR-0087。`~/.claude/scripts/base-moved.sh` が呼ぶ）。fetch するかの決め方（GitHub の
//! origin）と WARN の場合分けは、sh から呼ぶ `.claude/tests/scripts/base-moved.sh` が確かめる

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
            "base-moved-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("README.md", "init");
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

    fn commit(&self, path: &str, msg: &str) {
        let file = self.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, format!("{msg}\n")).unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    fn run(&self, args: &[&str]) -> Output {
        forget_git_env(&mut Command::new(env!("CARGO_BIN_EXE_claude-hooks")))
            .args(["script", "base-moved"])
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
fn a_base_that_moved_and_touched_the_same_paths_is_reported() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    repo.commit("src/b.txt", "b");
    repo.git(&["switch", "-q", "main"]);
    repo.commit("src/a.txt", "main a");
    repo.commit("src/c.txt", "main c");
    let fork = repo.git(&["merge-base", "main", "feat/work"]);
    let head = repo.git(&["rev-parse", "--short", "main"]);
    let out = repo.run(&["feat/work", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        format!(
            "BASE_MOVED: main が分岐点から 2 commits 進んでいる（先頭 {}、分岐点 {}）\nCOMMON: src/a.txt\n",
            head.trim(),
            &fork.trim()[..head.trim().len()]
        )
    );
    // リモートの無いリポジトリでも、ローカルのブランチの --base は確かめた比較として WARN を出さない
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn no_common_path_or_no_move_prints_nothing() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/work"]);
    repo.commit("src/a.txt", "a");
    let out = repo.run(&["--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "");
    repo.git(&["switch", "-q", "main"]);
    repo.commit("src/c.txt", "main c");
    let out = repo.run(&["feat/work", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn without_a_github_origin_or_base_it_does_nothing() {
    let repo = Repo::new();
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "");
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn bad_arguments_and_unknown_refs_are_errors() {
    let repo = Repo::new();
    for args in [
        &["--base"][..],
        &["--force"][..],
        &["no-such", "--base", "main"][..],
    ] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            text(&out.stderr).starts_with("ERROR: "),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}
