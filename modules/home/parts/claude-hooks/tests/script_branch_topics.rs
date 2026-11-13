//! `claude-hooks script branch-topics [base [head]]` が、ブランチが含むトピック（コミットの型、ADR と issue の追加と
//! status の変化、変えたディレクトリ）を要約することを確かめる（ADR-0087。`~/.claude/scripts/branch-topics.sh` が呼ぶ）。
//! 改名や README の扱いなどの場合分けは、sh から呼ぶ `.claude/tests/scripts/branch-topics.sh` も確かめる

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
            "branch-topics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Repo { root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.write(
            "docs/adr/0001-keep.md",
            "---\nstatus: proposed\n---\n\n# A\n",
        );
        repo.commit("docs: seed");
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
            .args(["script", "branch-topics"])
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn summarizes_the_topics_of_the_branch() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write(
        "docs/adr/0002-new.md",
        "---\nstatus: proposed\n---\n\n# B\n",
    );
    repo.commit("docs(adr): add b");
    repo.write(
        "docs/adr/0001-keep.md",
        "---\nstatus: accepted\n---\n\n# A\n",
    );
    repo.commit("docs(adr): accept a");
    repo.write("src/lib/a.rs", "a\n");
    repo.write("top.txt", "t\n");
    repo.commit("feat!: a");
    repo.write("x.txt", "x\n");
    repo.commit("wip");
    let out = repo.run(&["main"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "BASE: main\nHEAD: HEAD\nCOMMITS: 4\nTYPES: docs=2 feat=1 other=1\n\
         ADR_STATUS: docs/adr/0001-keep.md proposed -> accepted\n\
         ADR_NEW: docs/adr/0002-new.md (proposed)\n\
         DIRS: . docs/adr src/lib\n"
    );
}

#[test]
fn without_a_base_it_compares_with_the_default_branch() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("a.txt", "a\n");
    repo.commit("fix: a");
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).starts_with("BASE: main\nHEAD: HEAD\nCOMMITS: 1\nTYPES: fix=1\n"));
}

#[test]
fn bad_arguments_and_unknown_commits_are_errors() {
    let repo = Repo::new();
    let out = repo.run(&["a", "b", "c"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("Usage: branch-topics.sh"));
    let out = repo.run(&["no-such"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("コミットが見つかりません: no-such"));
}
