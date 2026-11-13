//! `claude-hooks git-hook staged-checks [言語]...` が、ステージした版（コミットに入る中身）の Nix、Python、Markdown、
//! Rust を、PATH のツールで検査することを確かめる（ADR-0028、ADR-0086。グローバルの pre-commit と run-checks.sh の
//! Markdown の段が呼ぶ）。ツールは偽のコマンド（examples/fake_tool.rs）を名前を変えて置く。本物のツールでの
//! Markdown の自動修正の書き戻しと Rust の edition は、modules/home/parts/tests/ の pre-commit-partial-stage.sh、
//! pre-commit-rust-format.sh、pre-commit-renamed-files.sh

mod common;

use common::*;
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

/// 呼ぶ git（PATH に本物のツールを混ぜないよう、git だけを偽のツールの置き場から見せる）
fn git_path() -> PathBuf {
    let probe = Command::new("git")
        .args(["--exec-path"])
        .output()
        .expect("git を起動できる");
    assert!(probe.status.success());
    let which = if cfg!(windows) { "where" } else { "which" };
    let out = Command::new(which)
        .arg("git")
        .output()
        .expect("git を探せる");
    let first = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .expect("git が PATH にある")
        .trim()
        .to_string();
    std::fs::canonicalize(first).unwrap()
}

impl Repo {
    fn new() -> Repo {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "staged-checks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("work")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        let repo = Repo { root };
        // Unix は git だけを置き場に symlink で置く。Windows の git の置き場（Git\cmd）には git しか無いので、そのまま足す
        #[cfg(unix)]
        std::os::unix::fs::symlink(git_path(), repo.root.join("bin/git")).unwrap();
        repo.git(&["init", "-q", "-b", "feat/x"]);
        repo
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn log(&self) -> PathBuf {
        self.root.join("tools.log")
    }

    /// 偽のツールを name で置く
    fn tool(&self, name: &str) {
        std::fs::copy(
            fake_tool(),
            self.root
                .join("bin")
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
    }

    fn path_env(&self) -> std::ffi::OsString {
        let mut dirs = vec![self.root.join("bin")];
        if cfg!(windows) {
            dirs.push(git_path().parent().unwrap().to_path_buf());
        }
        std::env::join_paths(dirs).unwrap()
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        forget_git_env(&mut cmd);
        cmd.current_dir(self.work())
            .env("PATH", self.path_env())
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("FAKE_TOOL_LOG", self.log());
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("FAKE_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("FAKE_TOOL_LOG", self.log());
        cmd
    }

    fn git(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd);
        let out = cmd
            .args(args)
            .current_dir(self.work())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn stage(&self, path: &str, text: &str) {
        let file = self.work().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        self.git(&["add", "--", path]);
    }

    fn checks(&self, langs: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = self.command(env!("CARGO_BIN_EXE_claude-hooks"));
        cmd.args(["git-hook", "staged-checks"]).args(langs);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
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
fn nothing_staged_passes_without_tools() {
    let repo = Repo::new();
    let out = repo.checks(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(repo.calls().is_empty());
}

#[test]
fn a_missing_tool_skips_its_language() {
    let repo = Repo::new();
    repo.stage("a.nix", "{ }\n");
    let out = repo.checks(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(
        said(&out).contains("SKIP: Nix の検査（nixfmt が PATH に無い）"),
        "{}",
        said(&out)
    );
}

#[test]
fn nixfmt_checks_the_staged_blob_on_stdin() {
    let repo = Repo::new();
    repo.tool("nixfmt");
    repo.stage("a.nix", "{ }\n");
    // 作業ツリーはステージした版と違ってよい（ステージした版を見る）
    std::fs::write(repo.work().join("a.nix"), "{a=1;}\n").unwrap();
    let stdin = repo.root.join("nixfmt.stdin");
    let out = repo.checks(
        &[],
        &[
            ("FAKE_NIXFMT_EXIT", "1"),
            ("FAKE_NIXFMT_STDIN", stdin.to_str().unwrap()),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        said(&out).contains("Nix format check failed: a.nix"),
        "{}",
        said(&out)
    );
    assert_eq!(repo.calls(), ["nixfmt --check -"]);
    assert_eq!(std::fs::read_to_string(&stdin).unwrap(), "{ }\n");
}

#[test]
fn ruff_formats_and_lints_with_the_staged_path() {
    let repo = Repo::new();
    repo.tool("ruff");
    repo.stage("pkg/a.py", "x = 1\n");
    let out = repo.checks(&[], &[("FAKE_RUFF_EXIT", "1")]);
    assert_eq!(out.status.code(), Some(1));
    let text = said(&out);
    assert!(
        text.contains("Python format check failed: pkg/a.py"),
        "{text}"
    );
    assert!(text.contains("Python lint failed"), "{text}");
    assert_eq!(
        repo.calls(),
        [
            "ruff format --check --stdin-filename pkg/a.py -",
            "ruff check --quiet --stdin-filename pkg/a.py -"
        ]
    );
}

#[test]
fn markdownlint_fixes_then_checks_a_copy_of_the_staged_files() {
    let repo = Repo::new();
    repo.tool("markdownlint");
    repo.stage("docs/a.md", "# a\n");
    let cwd = repo.root.join("markdownlint.cwd");
    let out = repo.checks(&[], &[("FAKE_MARKDOWNLINT_CWD", cwd.to_str().unwrap())]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(
        repo.calls(),
        [
            "markdownlint --fix --disable MD038 -- docs/a.md",
            "markdownlint -- docs/a.md"
        ]
    );
    // 作業ツリーでなく、ステージした版を書き出した場所で実行する
    for dir in std::fs::read_to_string(&cwd).unwrap().lines() {
        assert_ne!(
            std::fs::canonicalize(dir).ok(),
            std::fs::canonicalize(repo.work()).ok()
        );
    }
    // fix-markdown-lint.py（~/.claude の下）が無ければ飛ばす
    assert!(
        said(&out).contains("SKIP: fix-markdown-lint.py"),
        "{}",
        said(&out)
    );
}

#[test]
fn markdown_that_cannot_be_fixed_fails() {
    let repo = Repo::new();
    repo.tool("markdownlint");
    repo.stage("a.md", "# a\n");
    let out = repo.checks(&[], &[("FAKE_MARKDOWNLINT_EXIT", "1")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        said(&out).contains("Markdown lint failed"),
        "{}",
        said(&out)
    );
}

#[test]
fn rustfmt_outside_a_crate_uses_edition_2024_and_compares_the_output() {
    let repo = Repo::new();
    repo.tool("rustfmt");
    repo.stage("a.rs", "fn main() {}\n");
    let same = repo.checks(&[], &[("FAKE_RUSTFMT_ECHO", "1")]);
    assert_eq!(same.status.code(), Some(0), "{}", said(&same));
    assert_eq!(repo.calls(), ["rustfmt --edition 2024"]);
    let differs = repo.checks(
        &[],
        &[
            ("FAKE_RUSTFMT_ECHO", "1"),
            ("FAKE_RUSTFMT_OUT", "// changed\n"),
        ],
    );
    assert_eq!(differs.status.code(), Some(1));
    assert!(
        said(&differs).contains("Rust format check failed: a.rs"),
        "{}",
        said(&differs)
    );
}

#[test]
fn rustfmt_inside_a_crate_uses_the_package_edition_from_cargo_metadata() {
    let repo = Repo::new();
    repo.tool("rustfmt");
    repo.tool("cargo");
    repo.stage(
        "c/Cargo.toml",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    repo.stage("c/src/lib.rs", "pub fn f() {}\n");
    let manifest = std::fs::canonicalize(repo.work().join("c/Cargo.toml")).unwrap();
    let metadata =
        serde_json::json!({"packages": [{"manifest_path": manifest, "edition": "2021"}]});
    let out = repo.checks(
        &[],
        &[
            ("FAKE_RUSTFMT_ECHO", "1"),
            ("FAKE_CARGO_OUT", &metadata.to_string()),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let calls = repo.calls();
    assert!(
        calls.contains(&"rustfmt --edition 2021".to_string()),
        "{calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("cargo metadata --no-deps --format-version 1 --manifest-path ")),
        "{calls:?}"
    );
}

#[test]
fn named_languages_limit_the_checks() {
    let repo = Repo::new();
    repo.tool("nixfmt");
    repo.tool("markdownlint");
    repo.stage("a.nix", "{ }\n");
    repo.stage("a.md", "# a\n");
    let out = repo.checks(&["markdown"], &[("FAKE_NIXFMT_EXIT", "1")]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(
        repo.calls().iter().all(|c| c.starts_with("markdownlint ")),
        "{:?}",
        repo.calls()
    );
}

#[test]
fn an_unknown_language_is_a_usage_error() {
    let repo = Repo::new();
    let out = repo.checks(&["go"], &[]);
    assert_eq!(out.status.code(), Some(2));
}
