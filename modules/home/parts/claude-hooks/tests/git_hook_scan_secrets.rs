//! `claude-hooks git-hook scan-secrets` が、Claude Code のセッションのコミットで、コミットに入る中身を gitleaks で見て、
//! 漏えいと検査できないことを分けて止めることを確かめる（ADR-0015、ADR-0086。グローバルの pre-commit と
//! pre-merge-commit が呼ぶ）。gitleaks は偽のコマンド（examples/fake_tool.rs）にする。本物の gitleaks での検出は
//! modules/home/parts/tests/pre-commit-secrets.sh

mod common;

use common::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const LEAK: &str =
    r#"[{"File":"conf.txt","StartLine":3,"RuleID":"generic-api-key","Secret":"REDACTED"}]"#;

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
            "scan-secrets-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("work")).unwrap();
        let repo = Repo { root };
        let mut init = Command::new("git");
        forget_git_env(&mut init);
        let out = init
            .args(["init", "-q", "-b", "feat/x"])
            .current_dir(repo.work())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", repo.root.join("gitconfig"))
            .output()
            .unwrap();
        assert!(out.status.success());
        repo
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn log(&self) -> PathBuf {
        self.root.join("gitleaks.log")
    }

    /// git hook の中から呼ばれたのと同じく、リポジトリの直下で scan-secrets を実行する
    fn scan(&self, env: &[(&str, &str)], remove: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        forget_git_env(&mut cmd);
        cmd.args(["git-hook", "scan-secrets"])
            .current_dir(self.work())
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("CLAUDECODE", "1")
            .env("CLAUDE_HOOKS_GITLEAKS", fake_tool())
            .env("FAKE_TOOL_LOG", self.log())
            .env("FAKE_TOOL_OUT", "[]")
            .env_remove("FAKE_TOOL_EXIT")
            .env_remove("FAKE_TOOL_ERR")
            .env_remove("ALLOW_SECRET_COMMIT");
        for (k, v) in env {
            cmd.env(k, v);
        }
        for k in remove {
            cmd.env_remove(k);
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
fn a_clean_index_passes_after_one_staged_scan() {
    let repo = Repo::new();
    let out = repo.scan(&[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    // 値は出させず（--redact）、gitleaks の中の git diff の失敗を標準エラーで見分けられるよう、エラーだけを出させる
    assert_eq!(
        repo.calls(),
        [
            "git --staged --no-banner --redact --log-level error --report-format json --report-path - ."
        ]
    );
}

#[test]
fn leaks_stop_the_commit_with_places_and_rule_ids() {
    let repo = Repo::new();
    let out = repo.scan(&[("FAKE_TOOL_OUT", LEAK), ("FAKE_TOOL_EXIT", "1")], &[]);
    assert_eq!(out.status.code(), Some(1));
    let text = said(&out);
    assert!(text.contains("conf.txt:3 (generic-api-key)"), "{text}");
    assert!(!text.contains("REDACTED"), "{text}");
    assert!(text.contains("ALLOW_SECRET_COMMIT=<理由>"), "{text}");
}

#[test]
fn errors_on_stderr_mean_the_scan_did_not_run() {
    // gitleaks は中の git diff の失敗を ERR に出すだけで、終了コード 0 を返す
    let repo = Repo::new();
    let out = repo.scan(&[("FAKE_TOOL_ERR", "ERR git diff failed\n")], &[]);
    assert_eq!(out.status.code(), Some(1));
    let text = said(&out);
    assert!(text.contains("gitleaks が失敗しました"), "{text}");
    assert!(text.contains("git diff failed"), "{text}");
}

#[test]
fn git_warnings_mean_the_scan_stopped() {
    // gitleaks 8.30.1 は git diff の警告（CRLF の変換など）を、色を付けた ERR [git] warning: の行と、それに付ける
    // stderr is not empty の行で出し、差分を読むのをやめて 0 と空の一覧を返す（scratchpad の一時のリポジトリで本物の
    // 出力をコピーした。同じトークンが、警告の出ない LF の差分では見つかり、警告の出る差分では見つからない）
    let repo = Repo::new();
    let err = "\x1b[90m8:39PM\x1b[0m \x1b[31mERR\x1b[0m \x1b[1m[git] warning: in the working copy of 'a.txt', CRLF will be replaced by LF the next time Git touches it\x1b[0m\n\x1b[90m8:39PM\x1b[0m \x1b[31mERR\x1b[0m \x1b[36merror=\x1b[0m\x1b[31m\x1b[1m\"stderr is not empty\"\x1b[0m\x1b[0m\n";
    let out = repo.scan(&[("FAKE_TOOL_ERR", err)], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    let text = said(&out);
    // 色の符号を外して見せる
    assert!(
        text.contains("ERR [git] warning: in the working copy"),
        "{text}"
    );
    assert!(!text.contains('\x1b'), "{text}");
}

#[test]
fn a_failure_after_exit_zero_is_not_called_an_exit_failure() {
    let repo = Repo::new();
    let out = repo.scan(&[("FAKE_TOOL_ERR", "ERR [git] fatal: bad object\n")], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!said(&out).contains("exit 0"), "{}", said(&out));
}

#[test]
fn an_exit_of_one_without_findings_is_a_failure_not_a_leak() {
    // gitleaks は設定を読めないなどの失敗でも 1 を返す
    let repo = Repo::new();
    let out = repo.scan(&[("FAKE_TOOL_OUT", ""), ("FAKE_TOOL_EXIT", "1")], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        said(&out).contains("gitleaks が失敗しました"),
        "{}",
        said(&out)
    );
}

#[test]
fn commits_outside_claude_code_are_not_scanned() {
    let repo = Repo::new();
    let out = repo.scan(&[("FAKE_TOOL_EXIT", "1")], &["CLAUDECODE"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(repo.calls().is_empty());
}

#[test]
fn a_reason_in_allow_secret_commit_skips_the_scan() {
    let repo = Repo::new();
    let out = repo.scan(
        &[("ALLOW_SECRET_COMMIT", "テスト用"), ("FAKE_TOOL_EXIT", "1")],
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(repo.calls().is_empty());
}

#[test]
fn a_short_allow_secret_commit_still_scans_and_asks_for_a_reason() {
    let repo = Repo::new();
    let out = repo.scan(
        &[
            ("ALLOW_SECRET_COMMIT", "1"),
            ("FAKE_TOOL_OUT", LEAK),
            ("FAKE_TOOL_EXIT", "1"),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        said(&out).contains("ALLOW_SECRET_COMMIT には理由を記載"),
        "{}",
        said(&out)
    );
}

#[test]
fn blank_and_zero_values_count_as_no_escape() {
    let repo = Repo::new();
    for value in ["", "0", " \u{3000}\\ "] {
        let out = repo.scan(
            &[
                ("ALLOW_SECRET_COMMIT", value),
                ("FAKE_TOOL_OUT", LEAK),
                ("FAKE_TOOL_EXIT", "1"),
            ],
            &[],
        );
        assert_eq!(out.status.code(), Some(1), "{value:?}");
        assert!(
            !said(&out).contains("理由を記載"),
            "{value:?}: {}",
            said(&out)
        );
    }
}

#[test]
fn the_whole_repository_is_scanned_from_a_subdirectory() {
    // gitleaks は `.` を作業ディレクトリからの相対で読み、その下の変更だけを見る。サブディレクトリから呼んでも、
    // リポジトリの直下で調べる
    let repo = Repo::new();
    let sub = repo.work().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let cwd_log = repo.root.join("gitleaks.cwd");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
    forget_git_env(&mut cmd);
    let out = cmd
        .args(["git-hook", "scan-secrets"])
        .current_dir(&sub)
        .env("GIT_CEILING_DIRECTORIES", &repo.root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", repo.root.join("gitconfig"))
        .env("CLAUDECODE", "1")
        .env("CLAUDE_HOOKS_GITLEAKS", fake_tool())
        .env("FAKE_TOOL_OUT", "[]")
        .env("FAKE_TOOL_CWD", &cwd_log)
        .env_remove("ALLOW_SECRET_COMMIT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let ran_in = std::fs::read_to_string(&cwd_log).unwrap();
    assert_eq!(
        std::fs::canonicalize(ran_in.trim()).unwrap(),
        std::fs::canonicalize(repo.work()).unwrap()
    );
}

#[test]
fn a_failure_shows_gitleaks_errors_on_their_own_lines() {
    let repo = Repo::new();
    let out = repo.scan(&[("FAKE_TOOL_ERR", "ERR one\nERR two\n")], &[]);
    let text = said(&out);
    assert!(text.contains("\n   ERR one\n   ERR two"), "{text}");
}

// Windows は環境変数が無ければ PATH の gitleaks を使う（ADR-0086）
#[cfg(not(windows))]
#[test]
fn without_gitleaks_the_commit_stops() {
    let repo = Repo::new();
    let out = repo.scan(&[], &["CLAUDE_HOOKS_GITLEAKS"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(said(&out).contains("gitleaks"), "{}", said(&out));
}
