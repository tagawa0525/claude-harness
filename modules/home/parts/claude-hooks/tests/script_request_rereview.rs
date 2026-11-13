//! `claude-hooks script request-rereview <PR の URL>` が、head にローカルレビューの記録があるときだけ Copilot に
//! レビューを要求し、要求が登録されたら、その時刻からレビューを待つことを確かめる（ADR-0087。
//! `~/.claude/skills/gh-pr-review/scripts/request-rereview.sh` が呼ぶ）。@copilot のメンションでは再レビューは走らない
//! （応答する copilot-swe-agent はコメントを返すだけ）ので、要求は requested_reviewers の API だけで行い、失敗しても
//! メンションに逃げない。記録の判定（rebase だけの head を含む）は pre-merge-check と同じ関数で、その場合分けは
//! pre_merge_check.rs の単体テストが確かめる

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";
const HEAD: &str = "def5678def5678def5678def5678def5678def56";
const OLD: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
const BEFORE: &str = "2026-09-08T00:00:00Z\n";
const AFTER: &str = "2026-09-08T00:00:00Z\n2026-09-08T01:00:00Z\n";
const REVIEWS: &str = r#"{"reviews":[{"author":{"login":"copilot-pull-request-reviewer"},"state":"COMMENTED","submittedAt":"2026-09-08T01:05:00Z"}]}"#;
const NO_WAIT: &[(&str, &str)] = &[
    ("GH_WAIT_INTERVALS", "0"),
    ("GH_WAIT_REQUEST_INTERVALS", "0"),
];

fn record(sha: &str, high: u32) -> String {
    format!("<!-- local-review sha={sha} high={high} medium=0 low=0 -->\n")
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 要求の前の要求の一覧、head、記録、要求、要求の後の一覧、（待ち）gh の認証、PR、レビューの一覧
fn calls<'a>(records: &'a str, post_exit: i32, after: &'a str) -> Vec<(&'a str, i32)> {
    vec![
        (BEFORE, 0),
        (HEAD, 0),
        (records, 0),
        ("{}", post_exit),
        (after, 0),
        ("", 0),
        ("{\"number\":42}", 0),
        (REVIEWS, 0),
    ]
}

#[test]
fn requests_a_review_from_copilot_and_waits_from_the_request() {
    let fake = FakeGh::new();
    let rec = record(HEAD, 0);
    let out = fake.run_calls_with_env("request-rereview", &[URL], &calls(&rec, 0, AFTER), NO_WAIT);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let log = fake.log();
    assert!(
        log.contains(
            "gh api -X POST repos/{owner}/{repo}/pulls/42/requested_reviewers -f reviewers[]=copilot-pull-request-reviewer[bot]\n"
        ),
        "{log}"
    );
    assert!(!log.contains("@copilot"), "{log}");
    let text = stdout(&out);
    assert!(
        text.starts_with(
            "REQUESTED: copilot-pull-request-reviewer[bot]\nSINCE: 2026-09-08T01:00:00Z\n\
             INFO: 2026-09-08T01:00:00Z より新しいレビューの到着を待ちます\nOK: "
        ),
        "{text}"
    );
}

#[test]
fn no_record_of_the_head_requests_nothing_and_shows_how_to_record() {
    for records in [String::new(), record(HEAD, 1), record(OLD, 0)] {
        let fake = FakeGh::new();
        let out = fake.run_calls_with_env(
            "request-rereview",
            &[URL],
            &calls(&records, 0, AFTER),
            NO_WAIT,
        );
        assert_eq!(out.status.code(), Some(1), "{records}");
        assert!(!fake.log().contains("requested_reviewers"), "{records}");
        let err = stderr(&out);
        assert!(
            err.contains(&format!("record-local-review.sh {URL} ")),
            "{records}: {err}"
        );
        // REVIEW_FAILED の立て直しでも同じ道を通る。decide-next.sh に戻すと REVIEW_FAILED が続いて巡る
        assert!(
            err.contains(&format!("request-rereview.sh {URL}")),
            "{records}: {err}"
        );
        assert!(!err.contains("decide-next.sh"), "{records}: {err}");
    }
}

#[test]
fn a_request_that_fails_does_not_fall_back_or_wait() {
    let fake = FakeGh::new();
    let rec = record(HEAD, 0);
    let out = fake.run_calls_with_env("request-rereview", &[URL], &calls(&rec, 1, AFTER), NO_WAIT);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("レビューを要求できませんでした"),
        "{}",
        stderr(&out)
    );
    assert!(!fake.log().contains("@copilot"));
    assert!(!fake.log().contains("--json number"));
}

#[test]
fn a_request_that_is_not_registered_stops_before_waiting() {
    let fake = FakeGh::new();
    let rec = record(HEAD, 0);
    let out = fake.run_calls_with_env("request-rereview", &[URL], &calls(&rec, 0, BEFORE), NO_WAIT);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("登録されませんでした"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains(&format!("/gh-actions-check {URL} ")),
        "{}",
        stderr(&out)
    );
    assert!(!fake.log().contains("--json number"));
}

#[test]
fn a_failed_query_stops_without_requesting() {
    for calls in [
        // 要求の一覧を取れなければ、登録を確かめられない
        vec![("", 1)],
        // head を取れなければ、記録を確かめられない
        vec![(BEFORE, 0), ("", 0)],
        // 記録を取れなければ、記録が無いとみなさず止める
        vec![(BEFORE, 0), (HEAD, 0), ("", 1)],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls_with_env("request-rereview", &[URL], &calls, NO_WAIT);
        assert_eq!(out.status.code(), Some(1), "{calls:?}");
        assert!(stderr(&out).contains("ERROR: "), "{calls:?}");
        assert!(!fake.log().contains("requested_reviewers"), "{calls:?}");
    }
}

#[test]
fn bad_arguments_stop_before_gh() {
    for args in [
        &[][..],
        &["1"][..],
        &[URL, "x"][..],
        &["https://github.com/octo/repo/pull/42#r1"][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls_with_env("request-rereview", args, &[("", 0)], NO_WAIT);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: request-rereview.sh"),
            "{args:?}"
        );
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
