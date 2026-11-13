//! `claude-hooks script review-level [branch] [--base BASE]` が、ローカルの /code-review に渡すレベルと対象を決める
//! ことを確かめる（ADR-0087。`~/.claude/scripts/review-level.sh` が呼ぶ）。文書の判定の細かい場合分け（ADR の
//! 「## 変更」の節、.claude の下の文書、移動）と比較元の決め方は、sh から呼ぶ `.claude/tests/scripts/review-level.sh` も
//! 確かめる

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
            "review-level-{}-{}",
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
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&self.root)
            // 利用者のグローバルの設定（core.hooksPath のグローバルの git hook など）を読まない
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
            .args(["script", "review-level"])
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn documents_only_is_medium() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/docs"]);
    repo.commit("docs/plans/x.md", "plan");
    repo.commit("docs/issues/y.md", "issue");
    let out = repo.run(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let root = repo.git(&["rev-parse", "--show-toplevel"]);
    assert_eq!(
        stdout(&out),
        format!(
            "BASE: main\nTARGET: feat/docs\nWORKTREE: {}\nFILES: 2\nLEVEL: medium\n",
            root.trim()
        )
    );
}

#[test]
fn a_file_that_is_not_a_document_is_high() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/mixed"]);
    repo.commit("docs/plans/x.md", "plan");
    repo.commit(".claude/scripts/x.sh", "script");
    let out = repo.run(&["feat/mixed", "--base", "main"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).ends_with("FILES: 2\nLEVEL: high\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn an_empty_diff_or_an_unknown_ref_is_an_error() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "feat/empty"]);
    let out = repo.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("差分が空"));
    let out = repo.run(&["no-such-branch"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no-such-branch を解決できません"));
}

#[test]
fn bad_arguments_are_errors() {
    let repo = Repo::new();
    for args in [&["--base"][..], &["--force"][..]] {
        let out = repo.run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stdout(&out), "", "{args:?}");
    }
}
