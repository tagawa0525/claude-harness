//! `claude-hooks script get-review-comments <PR の URL> [--unresolved]` が、PR のレビュースレッドのコメントを、スレッドの
//! resolve の状態と投稿者が bot か人間かを添えて、作った順の 1 つの配列で出すことを確かめる（ADR-0087。
//! `~/.claude/skills/gh-pr-review/scripts/get-review-comments.sh` が呼ぶ）。REST の /pulls/{n}/comments はスレッドの
//! resolve の状態を持たないので、GraphQL の reviewThreads から取る

mod common;

use common::fake_gh::{FakeGh, stderr};
use serde_json::{Value, json};

const URL: &str = "https://github.com/octo/repo/pull/42";

/// gh の `--paginate --jq '.data.repository.pullRequest.reviewThreads.nodes[]'` の出力（1 行に 1 スレッド）
fn threads() -> String {
    [
        json!({"id":"T1","isResolved":false,"isOutdated":false,"path":"a.txt","line":3,
          "comments":{"pageInfo":{"hasNextPage":false},"nodes":[
            {"databaseId":11,"id":"C1","body":"fix this","author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"},
             "createdAt":"2026-09-01T00:00:00Z","url":"u1","replyTo":null},
            {"databaseId":13,"id":"C3","body":"Fixed","author":{"__typename":"User","login":"me"},
             "createdAt":"2026-09-01T00:02:00Z","url":"u3","replyTo":{"databaseId":11}}]}}),
        json!({"id":"T2","isResolved":true,"isOutdated":true,"path":"b.txt","line":null,
          "comments":{"pageInfo":{"hasNextPage":false},"nodes":[
            {"databaseId":22,"id":"C2","body":"why?","author":null,
             "createdAt":"2026-09-01T00:01:00Z","url":"u2","replyTo":null}]}}),
    ]
    .iter()
    .map(|t| format!("{t}\n"))
    .collect()
}

fn stdout_json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "標準出力が JSON でない（{e}）: {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn ids(out: &std::process::Output) -> Vec<u64> {
    stdout_json(out)
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_u64().unwrap())
        .collect()
}

#[test]
fn reads_the_threads_of_the_pr_in_the_repository_of_the_url() {
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL], &threads(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let log = fake.log();
    // owner と name は gh の {owner} / {repo} に解決させる（GH_REPO の URL のリポジトリ）
    assert!(
        log.starts_with(
            "gh api graphql --paginate -F owner={owner} -F name={repo} -F number=42 -f query="
        ),
        "{log}"
    );
    assert!(
        log.ends_with("--jq .data.repository.pullRequest.reviewThreads.nodes[]\n"),
        "{log}"
    );
    assert_eq!(fake.env(), "GH_REPO=octo/repo\nGH_HOST=github.com\n");
}

#[test]
fn each_comment_carries_its_thread_and_author() {
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL], &threads(), 0);
    let comments = stdout_json(&out);
    assert_eq!(
        comments[0],
        json!({"id":11,"node_id":"C1","path":"a.txt","line":3,"body":"fix this",
          "user":"copilot-pull-request-reviewer","user_type":"Bot","created_at":"2026-09-01T00:00:00Z",
          "in_reply_to_id":null,"html_url":"u1","thread_id":"T1","is_resolved":false,"is_outdated":false})
    );
    // 消えたアカウント（author が null）は user も user_type も null
    assert_eq!(
        comments[1],
        json!({"id":22,"node_id":"C2","path":"b.txt","line":null,"body":"why?",
          "user":null,"user_type":null,"created_at":"2026-09-01T00:01:00Z",
          "in_reply_to_id":null,"html_url":"u2","thread_id":"T2","is_resolved":true,"is_outdated":true})
    );
    assert_eq!(comments[2]["in_reply_to_id"], json!(11));
    assert_eq!(comments[2]["user_type"], json!("User"));
}

#[test]
fn the_comments_of_all_threads_are_in_the_order_they_were_made() {
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL], &threads(), 0);
    assert_eq!(ids(&out), [11, 22, 13]);
}

#[test]
fn unresolved_leaves_out_the_resolved_threads() {
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL, "--unresolved"], &threads(), 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(ids(&out), [11, 13]);
}

#[test]
fn no_threads_is_an_empty_array() {
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL], "", 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout_json(&out), json!([]));
}

#[test]
fn a_thread_with_more_than_100_comments_warns_that_the_tail_is_left_out() {
    let threads = threads().replacen(r#""hasNextPage":false"#, r#""hasNextPage":true"#, 1);
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL], &threads, 0);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).starts_with("WARN: "), "{}", stderr(&out));
    assert_eq!(ids(&out), [11, 22, 13]);
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code_and_nothing_is_printed() {
    let fake = FakeGh::new();
    let out = fake.run("get-review-comments", &[URL], "", 4);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn output_of_gh_that_is_not_threads_is_an_error() {
    for gh_out in ["not json\n", "[1]\n", "{\"id\":\"T1\"}\n"] {
        let fake = FakeGh::new();
        let out = fake.run("get-review-comments", &[URL], gh_out, 0);
        assert_eq!(out.status.code(), Some(1), "{gh_out}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "", "{gh_out}");
    }
}

#[test]
fn bad_arguments_stop_before_gh() {
    for args in [
        &[][..],
        &["1"][..],
        &["https://github.com/octo/repo/pull/42#discussion_r11"][..],
        &[URL, "--all"][..],
        &[URL, "--unresolved", "x"][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run("get-review-comments", args, &threads(), 0);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
