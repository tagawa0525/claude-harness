//! `claude-hooks script get-pr-info <PR の URL | コメントの URL>` が、URL のリポジトリの PR の情報を gh で取り、
//! コメントの URL ならその comment_id を添えて出すことを確かめる（ADR-0087。
//! `~/.claude/skills/gh-pr-review/scripts/get-pr-info.sh` が呼ぶ）。gh は偽のコマンド（examples/fake_tool.rs）を
//! PATH の先頭に置く

mod common;

use common::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fake {
    root: PathBuf,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fake {
    fn new() -> Fake {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "get-pr-info-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let gh = root
            .join("bin")
            .join(format!("gh{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(fake_tool(), &gh).unwrap();
        executable(&gh);
        Fake { root }
    }

    fn run(&self, args: &[&str], gh_out: &str, gh_exit: i32) -> Output {
        let path = std::env::join_paths(
            std::iter::once(self.root.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["script", "get-pr-info"])
            .args(args)
            .current_dir(&self.root)
            .env("PATH", path)
            // 呼んだ側の環境の対象（GH_REPO、GH_HOST）に向かず、URL のリポジトリに向くことを確かめる
            .env("GH_REPO", "other/repo")
            .env("GH_HOST", "ghe.example.com")
            .env("FAKE_GH_LOG", self.root.join("gh.log"))
            .env("FAKE_GH_ENV", self.root.join("gh-env.log"))
            .env("FAKE_GH_ENV_NAMES", "GH_REPO,GH_HOST")
            .env("FAKE_GH_OUT", gh_out)
            .env("FAKE_GH_EXIT", gh_exit.to_string())
            .output()
            .unwrap()
    }

    fn gh_log(&self) -> String {
        std::fs::read_to_string(self.root.join("gh.log")).unwrap_or_default()
    }

    fn gh_env(&self) -> String {
        std::fs::read_to_string(self.root.join("gh-env.log")).unwrap_or_default()
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout_json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "標準出力が JSON でない（{e}）: {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

const PR_JSON: &str = "{\"number\":42,\"title\":\"t\"}\n";

#[test]
fn reads_the_pr_in_the_repository_of_the_url() {
    let fake = Fake::new();
    let out = fake.run(&["https://github.com/octo/repo/pull/42"], PR_JSON, 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        fake.gh_log(),
        "gh pr view 42 --json number,title,url,state,reviewDecision,headRefName,baseRefName\n"
    );
    assert_eq!(fake.gh_env(), "GH_REPO=octo/repo\nGH_HOST=github.com\n");
}

#[test]
fn a_pr_url_adds_a_null_comment_id_to_the_pr() {
    let fake = Fake::new();
    let out = fake.run(&["https://github.com/octo/repo/pull/42"], PR_JSON, 0);
    assert_eq!(
        stdout_json(&out),
        serde_json::json!({"number": 42, "title": "t", "comment_id": null})
    );
}

#[test]
fn a_review_comment_url_adds_its_comment_id_as_a_number() {
    let fake = Fake::new();
    let out = fake.run(
        &["https://github.com/octo/repo/pull/42#discussion_r123456"],
        PR_JSON,
        0,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout_json(&out),
        serde_json::json!({"number": 42, "title": "t", "comment_id": 123456})
    );
}

#[test]
fn a_fragment_that_is_not_a_review_comment_is_the_whole_pr() {
    // #issuecomment-<id> などは、返信の API（pulls/comments）で返信できるコメントでない
    for url in [
        "https://github.com/octo/repo/pull/42#issuecomment-1",
        "https://github.com/octo/repo/pull/42#discussion_rabc",
        "https://github.com/octo/repo/pull/42#",
    ] {
        let fake = Fake::new();
        let out = fake.run(&[url], PR_JSON, 0);
        assert_eq!(out.status.code(), Some(0), "{url}: {}", stderr(&out));
        assert_eq!(
            stdout_json(&out)["comment_id"],
            serde_json::Value::Null,
            "{url}"
        );
    }
}

#[test]
fn a_page_or_query_under_the_pr_is_the_same_pr() {
    for url in [
        "https://github.com/octo/repo/pull/42/files",
        "https://github.com/octo/repo/pull/42/files#r123",
        "https://github.com/octo/repo/pull/42?notification_referrer_id=x",
    ] {
        let fake = Fake::new();
        let out = fake.run(&[url], PR_JSON, 0);
        assert_eq!(out.status.code(), Some(0), "{url}: {}", stderr(&out));
        assert!(fake.gh_log().starts_with("gh pr view 42 "), "{url}");
    }
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code_and_nothing_is_printed() {
    let fake = Fake::new();
    let out = fake.run(&["https://github.com/octo/repo/pull/42"], "", 4);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn output_of_gh_that_is_not_a_json_object_is_an_error() {
    for gh_out in ["not json", "[1]"] {
        let fake = Fake::new();
        let out = fake.run(&["https://github.com/octo/repo/pull/42"], gh_out, 0);
        assert_eq!(out.status.code(), Some(1), "{gh_out}");
        assert!(
            stderr(&out).contains("gh pr view"),
            "{gh_out}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn a_first_argument_that_is_not_a_pr_url_stops_before_gh() {
    // owner と repo は案内のコマンドに書くので、GitHub の名前に使えない文字（; や空白）も拒む
    for url in [
        "",
        "1",
        "https://github.com/octo/repo/issues/1",
        "https://example.com/octo/repo/pull/1",
        "https://github.com/evil;echo_PWN/repo/pull/1",
        "https://github.com/octo/re po/pull/1",
        // `.` と `..` は gh api の {owner}/{repo} に入り、API のパスを上に辿る
        "https://github.com/../x/pull/1",
        "https://github.com/octo/./pull/1",
    ] {
        let fake = Fake::new();
        let out = fake.run(&[url], PR_JSON, 0);
        assert_eq!(out.status.code(), Some(1), "{url}");
        assert!(
            stderr(&out).contains("PR の URL"),
            "{url}: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("Usage: get-pr-info.sh"),
            "{url}: {}",
            stderr(&out)
        );
        assert_eq!(fake.gh_log(), "", "{url}");
    }
}

#[test]
fn no_argument_shows_the_usage() {
    let fake = Fake::new();
    let out = fake.run(&[], PR_JSON, 0);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Usage: get-pr-info.sh"));
    assert_eq!(fake.gh_log(), "");
}
