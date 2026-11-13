//! `claude-hooks git-hook pre-merge-commit` を、グローバルの core.hooksPath に置いた hook のファイル（中身を呼ぶだけの
//! sh。ADR-0086）から git merge で呼び、main / master へのマージコミットを、機密情報の検査とマージ結果の品質チェック
//! （~/.claude の run-checks.sh --merge）に通ったときだけ作ることを確かめる。run-checks.sh と gitleaks は偽のものに
//! する。本物の run-checks.sh での判定と、.gitignore から外れた生成物の印は
//! modules/home/parts/tests/pre-merge-commit-checks.sh

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

impl Repo {
    fn new() -> Repo {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "pre-merge-commit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let hooks = root.join("global-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-merge-commit");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nexec \"{}\" git-hook pre-merge-commit \"$@\"\n",
                sh_path(Path::new(env!("CARGO_BIN_EXE_claude-hooks")))
            ),
        )
        .unwrap();
        executable(&hook);
        std::fs::write(
            root.join("gitconfig"),
            format!("[core]\n\thooksPath = {}\n", sh_path(&hooks)),
        )
        .unwrap();
        let repo = Repo { root };
        repo.run_checks(0);
        repo.git(&["init", "-q", "-b", "main", "work"], &repo.root);
        std::fs::write(repo.work().join("a.txt"), "a\n").unwrap();
        repo.git(&["add", "a.txt"], &repo.work());
        repo.git(&["commit", "-q", "-m", "chore: a"], &repo.work());
        repo.git(&["switch", "-q", "-c", "feat/x"], &repo.work());
        std::fs::write(repo.work().join("b.txt"), "b\n").unwrap();
        repo.git(&["add", "b.txt"], &repo.work());
        repo.git(&["commit", "-q", "-m", "feat: b"], &repo.work());
        repo.git(&["switch", "-q", "main"], &repo.work());
        repo
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn log(&self) -> PathBuf {
        self.root.join("tools.log")
    }

    /// ~/.claude の run-checks.sh の代わり（呼ばれた引数を記録し、code で終わる）
    fn run_checks(&self, code: i32) {
        let dir = self
            .root
            .join("home/.claude/skills/language-checks/scripts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("run-checks.sh"),
            format!(
                "echo \"run-checks $*\" >> \"{}\"\nexit {code}\n",
                sh_path(&self.log())
            ),
        )
        .unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        forget_git_env(&mut cmd);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("FAKE_") {
                cmd.env_remove(key);
            }
        }
        cmd.args(args)
            .current_dir(self.work())
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env("CLAUDECODE", "1")
            .env("CLAUDE_HOOKS_GITLEAKS", fake_tool())
            .env("FAKE_TOOL_LOG", self.log())
            .env("FAKE_TOOL_OUT", "[]")
            .env_remove("ALLOW_SECRET_COMMIT");
        cmd
    }

    fn git(&self, args: &[&str], dir: &Path) -> Output {
        let out = self.command(args).current_dir(dir).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn merge(&self) -> Output {
        self.command(&["merge", "-q", "--no-ff", "--no-edit", "feat/x"])
            .output()
            .unwrap()
    }

    fn merged(&self) -> bool {
        self.command(&["rev-parse", "-q", "--verify", "HEAD^2"])
            .output()
            .unwrap()
            .status
            .success()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

fn said(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const STAGED_SCAN: &str =
    "git --staged --no-banner --redact --log-level error --report-format json --report-path - .";

#[test]
fn a_merge_into_main_that_passes_the_checks_is_committed() {
    let repo = Repo::new();
    let out = repo.merge();
    assert!(out.status.success(), "{}", said(&out));
    assert!(repo.merged());
    assert_eq!(repo.calls(), [STAGED_SCAN, "run-checks --merge"]);
}

#[test]
fn a_merge_that_fails_the_checks_is_not_committed() {
    let repo = Repo::new();
    repo.run_checks(1);
    let out = repo.merge();
    assert!(!out.status.success());
    assert!(!repo.merged());
    assert!(
        said(&out).contains("マージ結果が品質チェックに通らないので"),
        "{}",
        said(&out)
    );
}

#[test]
fn merges_into_other_branches_only_get_the_secret_scan() {
    let repo = Repo::new();
    repo.git(&["switch", "-q", "-c", "other"], &repo.work());
    let out = repo.merge();
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(repo.calls(), [STAGED_SCAN]);
}

#[test]
fn changes_outside_the_merge_stop_it_and_are_listed() {
    let repo = Repo::new();
    std::fs::write(repo.work().join("a.txt"), "edited\n").unwrap();
    std::fs::write(repo.work().join("new.txt"), "n\n").unwrap();
    let out = repo.merge();
    assert!(!out.status.success());
    let text = said(&out);
    assert!(
        text.contains("作業ツリーにマージ結果以外の変更があるので"),
        "{text}"
    );
    assert!(text.contains("a.txt") && text.contains("new.txt"), "{text}");
    assert!(!repo.calls().iter().any(|c| c.starts_with("run-checks")));
}

#[test]
fn a_missing_run_checks_stops_the_merge() {
    let repo = Repo::new();
    std::fs::remove_file(
        repo.root
            .join("home/.claude/skills/language-checks/scripts/run-checks.sh"),
    )
    .unwrap();
    let out = repo.merge();
    assert!(!out.status.success());
    assert!(
        said(&out).contains("run-checks.sh がないので"),
        "{}",
        said(&out)
    );
}

#[test]
fn forks_skip_the_quality_checks() {
    let repo = Repo::new();
    repo.git(
        &["remote", "add", "upstream", "https://example.com/u/r.git"],
        &repo.work(),
    );
    let out = repo.merge();
    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("fork"), "{}", said(&out));
    assert_eq!(repo.calls(), [STAGED_SCAN]);
}

#[test]
fn a_local_pre_merge_commit_runs_first_and_its_failure_stops_the_merge() {
    let repo = Repo::new();
    let hooks = repo.work().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let local = hooks.join("pre-merge-commit");
    std::fs::write(
        &local,
        format!(
            "#!/bin/sh\necho local >> \"{}\"\nexit 3\n",
            sh_path(&repo.log())
        ),
    )
    .unwrap();
    executable(&local);
    let out = repo.merge();
    assert!(!out.status.success());
    assert_eq!(repo.calls(), ["local"]);
}
