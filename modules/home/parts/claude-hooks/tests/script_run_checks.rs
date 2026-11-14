//! `claude-hooks script run-checks` が、ステージ済み（`--merge` は HEAD と作業ツリー、`--base` は分岐点と作業ツリー）の
//! 差分と目印のファイルから言語を選び、段ごとに `RUN:` と `OK:` / `SKIP:` / `FAILED:`（あれば `FIX:`）を出し、すべて
//! 通れば `ALL_OK` を出すことを確かめる（ADR-0087。`~/.claude/skills/language-checks/scripts/run-checks.sh` が呼ぶ）。
//! 排他の Slurm のジョブ中の制限は ADR-0041。ツールは偽のコマンド（examples/fake_tool.rs）を名前を変えて置く。
//! 場合分けの残りは、sh から呼ぶ `.claude/tests/scripts/language-checks.sh` も確かめる

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

/// 呼ぶ git（PATH に本物のツールを混ぜないよう、git だけを偽のツールの置き場から見せる）。which は Nix の sandbox に
/// 無いので、PATH を自分で探す
fn git_path() -> PathBuf {
    let name = format!("git{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::var_os("PATH").expect("PATH がある");
    let found = std::env::split_paths(&path)
        .map(|dir| dir.join(&name))
        .find(|p| p.is_file())
        .expect("git が PATH にある");
    std::fs::canonicalize(found).unwrap()
}

impl Repo {
    /// main に最初のコミットを持つリポジトリ
    fn new() -> Repo {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "run-checks-{}-{}",
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
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("README", "init\n");
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
        place_fake_tool(
            &self
                .root
                .join("bin")
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        );
    }

    fn path_env(&self) -> std::ffi::OsString {
        let mut dirs = vec![self.root.join("bin")];
        if cfg!(windows) {
            dirs.push(git_path().parent().unwrap().to_path_buf());
        }
        std::env::join_paths(dirs).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = forget_git_env(&mut Command::new("git"))
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
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
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.work().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }

    fn stage(&self, path: &str, text: &str) {
        self.write(path, text);
        self.git(&["add", "--", path]);
    }

    fn commit(&self, path: &str, text: &str) {
        self.stage(path, text);
        self.git(&["commit", "-q", "-m", path]);
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.run_in(&self.work(), args, env)
    }

    fn run_in(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        forget_git_env(&mut cmd);
        cmd.args(["script", "run-checks"])
            .args(args)
            .current_dir(dir)
            .env("PATH", self.path_env())
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("FAKE_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("FAKE_TOOL_LOG", self.log());
        cmd.envs(env.iter().copied());
        cmd.output().unwrap()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn said(out: &Output) -> String {
    format!("{}{}", stdout(out), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_python_project_runs_format_lint_and_test_in_order_then_all_ok() {
    let repo = Repo::new();
    repo.write("pyproject.toml", "");
    repo.write("tests/test_a.py", "");
    repo.tool("ruff");
    repo.tool("pytest");
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(
        stdout(&out),
        "RUN: python format: ruff format --check .\nOK: python format\n\
         RUN: python lint: ruff check .\nOK: python lint\n\
         RUN: python test: pytest\nOK: python test\nALL_OK\n"
    );
    assert_eq!(
        repo.calls(),
        ["ruff format --check .", "ruff check .", "pytest "]
    );
}

#[test]
fn a_failing_stage_stops_and_shows_the_fix() {
    let repo = Repo::new();
    repo.write("pyproject.toml", "");
    repo.tool("ruff");
    let out = repo.run(&[], &[("FAKE_RUFF_EXIT", "1")]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    let text = stdout(&out);
    assert!(
        text.ends_with("FAILED: python format: ruff format --check .\nFIX: ruff format .\n"),
        "{text}"
    );
    assert_eq!(repo.calls(), ["ruff format --check ."]);
}

#[test]
fn a_missing_tool_is_skipped_and_the_checks_go_on() {
    let repo = Repo::new();
    repo.write("pyproject.toml", "");
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let text = stdout(&out);
    assert!(
        text.contains("SKIP: python format (ruff not found)"),
        "{text}"
    );
    assert!(
        text.contains("SKIP: python lint (ruff not found)"),
        "{text}"
    );
    assert!(
        text.contains("SKIP: python test (テストが見つからない)"),
        "{text}"
    );
    assert!(text.ends_with("ALL_OK\n"), "{text}");
}

#[test]
fn only_the_staged_shell_scripts_go_to_shellcheck() {
    let repo = Repo::new();
    repo.commit("old.sh", "echo old\n");
    repo.tool("shellcheck");
    repo.stage("new.sh", "echo new\n");
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(stdout(&out).contains("RUN: shell lint: sh_lint\nOK: shell lint\n"));
    assert_eq!(repo.calls(), ["shellcheck -S warning -- new.sh"]);
}

#[test]
fn the_staged_markdown_goes_to_staged_checks_and_merge_only_lints_it() {
    let repo = Repo::new();
    repo.tool("staged-checks");
    repo.tool("markdownlint");
    repo.stage("doc.md", "# doc\n");
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(stdout(&out).contains("RUN: markdown staged: staged-checks markdown\n"));
    assert_eq!(repo.calls(), ["staged-checks markdown"]);

    std::fs::remove_file(repo.log()).unwrap();
    let out = repo.run(&["--merge"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let text = stdout(&out);
    assert!(
        text.contains(
            "SKIP: markdown format (--merge では自動修正しない)\nRUN: markdown lint: md_lint\n"
        ),
        "{text}"
    );
    assert_eq!(repo.calls(), ["markdownlint -- doc.md"]);
}

#[test]
fn rust_files_without_a_root_manifest_run_cargo_in_their_crate() {
    let repo = Repo::new();
    repo.commit("sub/app/Cargo.toml", "[package]\n");
    repo.tool("cargo");
    repo.stage("sub/app/src/lib.rs", "\n");
    repo.stage("loose.rs", "\n");
    let cwd = repo.root.join("cargo.cwd");
    let out = repo.run(
        &[],
        &[
            ("FAKE_CARGO_EXIT", "1"),
            ("FAKE_CARGO_CWD", cwd.to_str().unwrap()),
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(
        stdout(&out).ends_with(
            "RUN: rust format: cargo_in sub/app fmt --check\n\
             FAILED: rust format: cargo_in sub/app fmt --check\n\
             FIX: (cd -- sub/app && cargo fmt)\n"
        ),
        "{}",
        said(&out)
    );
    let ran_in = std::fs::read_to_string(&cwd).unwrap();
    assert!(
        Path::new(ran_in.trim()).ends_with(Path::new("sub").join("app")),
        "{ran_in}"
    );
}

#[test]
fn rust_files_outside_any_crate_are_skipped() {
    let repo = Repo::new();
    repo.tool("cargo");
    repo.stage("loose.rs", "\n");
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(
        stdout(&out),
        "SKIP: rust (Cargo.toml の無い場所の .rs: loose.rs)\nALL_OK\n"
    );
    assert!(repo.calls().is_empty());
}

#[test]
fn base_checks_the_committed_changes_of_the_branch_from_another_directory() {
    let repo = Repo::new();
    repo.git(&["checkout", "-q", "-b", "feat/x"]);
    repo.commit("a.sh", "echo a\n");
    repo.tool("shellcheck");
    let work = repo.work();
    let out = repo.run_in(
        &repo.root,
        &["-C", work.to_str().unwrap(), "--base", "main"],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(repo.calls(), ["shellcheck -S warning -- a.sh"]);
}

#[test]
fn an_unresolvable_base_and_unknown_options_are_errors() {
    let repo = Repo::new();
    let out = repo.run(&["--base", "nonexistent"], &[]);
    assert_eq!(out.status.code(), Some(2), "{}", said(&out));
    assert!(said(&out).contains("ERROR: --base nonexistent と HEAD の分岐点を求められません"));

    let out = repo.run(&["--bogus"], &[]);
    assert_eq!(out.status.code(), Some(2), "{}", said(&out));
    assert!(said(&out).contains("ERROR: 知らないオプション: --bogus"));

    let out = repo.run(&["--base"], &[]);
    assert_eq!(out.status.code(), Some(2), "{}", said(&out));
    assert!(said(&out).contains("ERROR: --base には引数が要ります"));
}

#[test]
fn no_language_is_a_note_and_success() {
    let repo = Repo::new();
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(stdout(&out).starts_with("NOTE: 対象言語が見つかりません"));
}

#[test]
fn a_design_records_opt_in_checks_every_changed_file() {
    let repo = Repo::new();
    repo.commit(
        ".pre-commit-config.yaml",
        "repos:\n  - repo: local\n    hooks:\n      - id: design-records # ADR\n",
    );
    repo.tool("design-records");
    repo.stage("notes.txt", "x\n");
    let out = repo.run(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(stdout(&out).contains("RUN: markdown design-records: design_records_check\n"));
    assert_eq!(repo.calls(), ["design-records check --changed notes.txt"]);
}

/// 排他のジョブ openmc-2K（1234）が r995 で走っている状態の squeue と、hostname を置く
fn exclusive_job_on(repo: &Repo) -> Vec<(&'static str, &'static str)> {
    repo.commit("Cargo.toml", "[package]\n");
    repo.commit("flake.nix", "{}\n");
    for tool in [
        "squeue", "hostname", "cargo", "taskset", "nix", "statix", "nixfmt",
    ] {
        repo.tool(tool);
    }
    vec![(
        "FAKE_SQUEUE_OUT",
        "NO|openmc-2K|1234|r995\nOK|x-shared|1235|r995\n",
    )]
}

#[test]
fn on_the_node_of_an_exclusive_job_cargo_and_nix_use_one_core() {
    let repo = Repo::new();
    let mut env = exclusive_job_on(&repo);
    env.push(("FAKE_HOSTNAME_OUT", "r995\n"));
    let out = repo.run(&[], &env);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(
        stdout(&out).starts_with(
            "排他のジョブ openmc-2K（1234）がこのホストで実行中なので 1 コアで実行する（遅い）\n"
        ),
        "{}",
        said(&out)
    );
    let calls = repo.calls();
    let pinned = |tail: &str| {
        calls
            .iter()
            .any(|c| c.starts_with("taskset -c ") && c.ends_with(tail))
    };
    assert!(pinned(" cargo fmt --check"), "{calls:?}");
    assert!(
        pinned(" cargo clippy -j 1 --all-targets -- -D warnings"),
        "{calls:?}"
    );
    assert!(pinned(" cargo test -j 1"), "{calls:?}");
    assert!(
        calls.contains(&"nix flake check --cores 1 -j 1".to_string()),
        "{calls:?}"
    );
}

#[test]
fn off_the_node_cargo_is_not_limited_and_nix_builds_here() {
    let repo = Repo::new();
    let mut env = exclusive_job_on(&repo);
    env.push(("FAKE_HOSTNAME_OUT", "t14g4.lan\n"));
    let out = repo.run(&[], &env);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(
        stdout(&out).starts_with(
            "排他のジョブ openmc-2K（1234）が計算ノードで実行中なので、nix のビルドは計算ノードに投げずに手元でビルドする\n"
        ),
        "{}",
        said(&out)
    );
    let calls = repo.calls();
    assert!(!calls.iter().any(|c| c.starts_with("taskset")), "{calls:?}");
    assert!(calls.contains(&"cargo test".to_string()), "{calls:?}");
    assert!(
        calls.contains(&"nix flake check --builders  --max-jobs auto".to_string()),
        "{calls:?}"
    );
}

#[test]
fn tracked_tests_under_a_non_ascii_directory_run_in_that_directory() {
    let repo = Repo::new();
    repo.write("pyproject.toml", "");
    repo.commit("実験/test_a.py", "");
    repo.tool("ruff");
    repo.tool("python3");
    let cwd = repo.root.join("python3.cwd");
    let out = repo.run(&[], &[("FAKE_PYTHON3_CWD", cwd.to_str().unwrap())]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(
        stdout(&out).contains("RUN: python test: cd 実験 && python3 -m pytest test_a.py\n"),
        "{}",
        said(&out)
    );
    let ran_in = std::fs::read_to_string(&cwd).unwrap();
    assert!(
        Path::new(ran_in.lines().last().unwrap()).ends_with("実験"),
        "{ran_in}"
    );
}

#[test]
fn an_empty_directory_option_checks_the_current_directory() {
    let repo = Repo::new();
    repo.tool("shellcheck");
    repo.stage("a.sh", "echo a\n");
    let out = repo.run(&["-C", ""], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(repo.calls(), ["shellcheck -S warning -- a.sh"]);
}
