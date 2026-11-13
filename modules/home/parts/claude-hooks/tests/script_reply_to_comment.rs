//! `claude-hooks script reply-to-comment <PR の URL> <comment_id> <body>` が、PR の URL のリポジトリに向けて
//! レビューコメントに返信することを確かめる（ADR-0087。`~/.claude/skills/gh-pr-review/scripts/reply-to-comment.sh` が
//! 呼ぶ）。gh は偽のコマンド（examples/fake_tool.rs）を PATH の先頭に置く

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
            "reply-to-comment-{}-{}",
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
            .args(["script", "reply-to-comment"])
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

#[test]
fn replies_to_the_comment_in_the_repository_of_the_url() {
    let fake = Fake::new();
    let out = fake.run(
        &[
            "https://github.com/octo/repo/pull/42",
            "111",
            "Fixed in abc.",
        ],
        "{\"id\":222}\n",
        0,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{\"id\":222}\n");
    assert_eq!(
        fake.gh_log(),
        "gh api repos/{owner}/{repo}/pulls/42/comments/111/replies -f body=Fixed in abc. \
         --jq {id: .id, html_url: .html_url, body: .body}\n"
    );
    assert_eq!(fake.gh_env(), "GH_REPO=octo/repo\nGH_HOST=github.com\n");
}

#[test]
fn a_page_or_query_under_the_pr_is_the_same_pr() {
    for url in [
        "https://github.com/octo/repo/pull/42/files",
        "https://github.com/octo/repo/pull/42?notification_referrer_id=x",
    ] {
        let fake = Fake::new();
        let out = fake.run(&[url, "111", "ok"], "", 0);
        assert_eq!(out.status.code(), Some(0), "{url}: {}", stderr(&out));
        assert!(
            fake.gh_log().contains("pulls/42/comments/111/replies"),
            "{url}"
        );
    }
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code() {
    let fake = Fake::new();
    let out = fake.run(
        &["https://github.com/octo/repo/pull/42", "111", "ok"],
        "",
        4,
    );
    assert_eq!(out.status.code(), Some(4));
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
        let out = fake.run(&[url, "111", "ok"], "", 0);
        assert_eq!(out.status.code(), Some(1), "{url}");
        assert!(
            stderr(&out).contains("PR の URL"),
            "{url}: {}",
            stderr(&out)
        );
        assert_eq!(fake.gh_log(), "", "{url}");
    }
}

#[test]
fn a_url_with_a_fragment_stops_before_gh() {
    // コメントの URL を渡すと、別に渡す comment_id と食い違っても気付けない
    for url in [
        "https://github.com/octo/repo/pull/1#discussion_r111",
        "https://github.com/octo/repo/pull/1/files#r111",
    ] {
        let fake = Fake::new();
        let out = fake.run(&[url, "222", "Fixed"], "", 0);
        assert_eq!(out.status.code(), Some(1), "{url}");
        assert!(stderr(&out).contains("#"), "{url}: {}", stderr(&out));
        assert_eq!(fake.gh_log(), "", "{url}");
    }
}

#[test]
fn a_missing_comment_id_or_body_shows_the_usage() {
    for args in [
        &["https://github.com/octo/repo/pull/1"][..],
        &["https://github.com/octo/repo/pull/1", "111"][..],
        &["https://github.com/octo/repo/pull/1", "", "body"][..],
        &["https://github.com/octo/repo/pull/1", "111", ""][..],
    ] {
        let fake = Fake::new();
        let out = fake.run(args, "", 0);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: reply-to-comment.sh"),
            "{args:?}"
        );
        assert_eq!(fake.gh_log(), "", "{args:?}");
    }
}

#[test]
fn a_comment_id_that_is_not_a_number_stops_before_gh() {
    // id は API のパスに埋めるので、数でない値（URL の断片、`..`、クエリ）は別のエンドポイントに当たる
    for id in ["abc", "111/../../../issues/1/comments", "111?x=", "-1"] {
        let fake = Fake::new();
        let out = fake.run(&["https://github.com/octo/repo/pull/1", id, "ok"], "", 0);
        assert_eq!(out.status.code(), Some(1), "{id}");
        assert!(
            stderr(&out).contains("comment_id"),
            "{id}: {}",
            stderr(&out)
        );
        assert_eq!(fake.gh_log(), "", "{id}");
    }
}

#[test]
fn extra_arguments_stop_before_gh() {
    // 本文を引用符で囲み忘れると、最初の語だけが返信になる
    let fake = Fake::new();
    let out = fake.run(
        &[
            "https://github.com/octo/repo/pull/1",
            "111",
            "Fixed",
            "in",
            "abc.",
        ],
        "",
        0,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Usage: reply-to-comment.sh"));
    assert_eq!(fake.gh_log(), "");
}
