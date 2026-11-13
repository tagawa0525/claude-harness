//! `claude-hooks script resolve-thread <PR の URL> <comment_id> [--allow-human]` が、コメントを含むレビュースレッドを
//! GraphQL で探して resolve し、人間が起こしたスレッドは既定で resolve しないことを確かめる（ADR-0087。
//! `~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh` が呼ぶ）。スレッドを閉じるのはレビュアーの権限で、bot の
//! スレッドは対応後に resolve し、人間のスレッドは返信だけにして相手に委ねる

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";
/// gh の `--jq '.data.repository.pullRequest.reviewThreads'` の出力
const THREADS: &str = r#"{"pageInfo":{"hasNextPage":false},"nodes":[
  {"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":11,"author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"}}]}},
  {"id":"T2","isResolved":false,"comments":{"nodes":[{"databaseId":22,"author":{"__typename":"User","login":"alice"}},{"databaseId":23,"author":{"__typename":"User","login":"me"}}]}},
  {"id":"T3","isResolved":true,"comments":{"nodes":[{"databaseId":33,"author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"}}]}},
  {"id":"T4","isResolved":false,"comments":{"nodes":[{"databaseId":44,"author":null}]}}
]}
"#;
const RESOLVED: &str = "resolved: T1 (isResolved=true)\n";

fn gh_calls(fake: &FakeGh) -> Vec<String> {
    // 起動ごとの行の先頭は `gh api graphql`（query は改行を含むので、行でなく起動の先頭で分ける）
    fake.log()
        .split("gh api graphql")
        .skip(1)
        .map(|call| format!("gh api graphql{call}"))
        .collect()
}

#[test]
fn resolves_the_bot_thread_that_has_the_comment() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "resolve-thread",
        &[URL, "11"],
        &[(THREADS, 0), (RESOLVED, 0)],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), RESOLVED);
    let calls = gh_calls(&fake);
    assert_eq!(calls.len(), 2, "{calls:?}");
    // owner と name は gh の {owner} / {repo} に解決させる（GH_REPO の URL のリポジトリ）
    assert!(
        calls[0]
            .starts_with("gh api graphql -F owner={owner} -F name={repo} -F number=42 -f query="),
        "{}",
        calls[0]
    );
    assert!(
        calls[0].ends_with("--jq .data.repository.pullRequest.reviewThreads\n"),
        "{}",
        calls[0]
    );
    assert!(
        calls[1].starts_with("gh api graphql -F id=T1 -f query="),
        "{}",
        calls[1]
    );
    assert!(calls[1].contains("resolveReviewThread"), "{}", calls[1]);
    assert_eq!(
        fake.env(),
        "GH_REPO=octo/repo\nGH_HOST=github.com\nGH_REPO=octo/repo\nGH_HOST=github.com\n"
    );
}

#[test]
fn a_reply_in_the_thread_finds_the_same_thread() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "resolve-thread",
        &[URL, "23", "--allow-human"],
        &[(THREADS, 0), ("", 0)],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(gh_calls(&fake)[1].starts_with("gh api graphql -F id=T2 "));
}

#[test]
fn a_thread_started_by_a_human_is_not_resolved_by_default() {
    // 先頭のコメントの投稿者がスレッドを起こした人。返信した人（23 の me）では決めない
    for id in ["22", "23"] {
        let fake = FakeGh::new();
        let out = fake.run_calls("resolve-thread", &[URL, id], &[(THREADS, 0), ("", 0)]);
        assert_eq!(out.status.code(), Some(1), "{id}");
        assert!(stderr(&out).contains("alice"), "{id}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("--allow-human"),
            "{id}: {}",
            stderr(&out)
        );
        assert_eq!(gh_calls(&fake).len(), 1, "{id}: mutation を呼ばない");
    }
}

#[test]
fn a_thread_whose_starter_is_unknown_is_not_resolved_by_default() {
    // 消えたアカウント（author が null）は bot と確かめられない
    let fake = FakeGh::new();
    let out = fake.run_calls("resolve-thread", &[URL, "44"], &[(THREADS, 0), ("", 0)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("unknown"), "{}", stderr(&out));
    assert_eq!(gh_calls(&fake).len(), 1);
}

#[test]
fn allow_human_resolves_a_thread_started_by_a_human() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "resolve-thread",
        &[URL, "22", "--allow-human"],
        &[(THREADS, 0), ("", 0)],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(gh_calls(&fake)[1].starts_with("gh api graphql -F id=T2 "));
}

#[test]
fn an_already_resolved_thread_is_reported_without_the_mutation() {
    let fake = FakeGh::new();
    let out = fake.run_calls("resolve-thread", &[URL, "33"], &[(THREADS, 0), ("", 0)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "already resolved: T3\n"
    );
    assert_eq!(gh_calls(&fake).len(), 1);
}

#[test]
fn a_comment_in_no_thread_is_an_error() {
    let fake = FakeGh::new();
    let out = fake.run_calls("resolve-thread", &[URL, "99"], &[(THREADS, 0), ("", 0)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("99"), "{}", stderr(&out));
    assert!(stderr(&out).contains("#42"), "{}", stderr(&out));
    assert_eq!(gh_calls(&fake).len(), 1);
}

#[test]
fn more_than_100_threads_is_an_error() {
    // 101 件目より後のスレッドにあるコメントを、見つからないと言い誤らない
    let threads = THREADS.replace(r#""hasNextPage":false"#, r#""hasNextPage":true"#);
    let fake = FakeGh::new();
    let out = fake.run_calls("resolve-thread", &[URL, "11"], &[(&threads, 0), ("", 0)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("100"), "{}", stderr(&out));
    assert_eq!(gh_calls(&fake).len(), 1);
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code() {
    for calls in [&[("", 4)][..], &[(THREADS, 0), ("", 5)][..]] {
        let fake = FakeGh::new();
        let out = fake.run_calls("resolve-thread", &[URL, "11"], calls);
        assert_eq!(
            out.status.code(),
            Some(calls.last().unwrap().1),
            "{calls:?}"
        );
    }
}

#[test]
fn output_of_gh_that_is_not_the_threads_is_an_error() {
    for threads in ["not json", "[]", r#"{"nodes":[]}"#] {
        let fake = FakeGh::new();
        let out = fake.run_calls("resolve-thread", &[URL, "11"], &[(threads, 0), ("", 0)]);
        assert_eq!(out.status.code(), Some(1), "{threads}");
        assert_eq!(gh_calls(&fake).len(), 1, "{threads}");
    }
}

#[test]
fn bad_arguments_stop_before_gh() {
    for args in [
        &[][..],
        &["1", "11"][..],
        &["https://github.com/octo/repo/pull/42#discussion_r11", "11"][..],
        &[URL][..],
        &[URL, ""][..],
        // 数でない値（URL の断片の discussion_r11 など）
        &[URL, "discussion_r11"][..],
        &[URL, "11", "--force"][..],
        &[URL, "11", "--allow-human", "x"][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls("resolve-thread", args, &[(THREADS, 0)]);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
