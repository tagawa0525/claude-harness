//! `claude-hooks script gh-wait-review <PR の URL> [--since <時刻>]` が、PR に Copilot のレビューが提出されるのを
//! 待つことを確かめる（ADR-0087。`~/.claude/scripts/gh-wait-review.sh` が呼び、request-rereview.sh も要求の後に呼ぶ）。
//! 待ち方の細かい場合分け（要求の記録を待つ、作成者のレビューで打ち切らない、待つたびに head を取り直す）は、sh から
//! 呼ぶ `.claude/tests/scripts/gh-pr-review.sh` も確かめる

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";
const REVIEWS: &str = r#"{"reviews":[{"author":{"login":"copilot-pull-request-reviewer"},"state":"COMMENTED","submittedAt":"2026-09-08T03:00:00Z"},{"author":null,"state":"COMMENTED","submittedAt":"2026-09-08T04:00:00Z"},{"author":{"login":"me"},"state":"COMMENTED","submittedAt":"2026-09-08T05:00:00Z"}]}"#;
/// 待つ間隔を 0 にする（既定は約 10 分）
const NO_WAIT: &[(&str, &str)] = &[
    ("GH_WAIT_INTERVALS", "0"),
    ("GH_WAIT_REQUEST_INTERVALS", "0"),
];

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_copilot_review_after_since_is_the_review_it_waits_for() {
    // 作成者の返信（me）と author の無いレビューの時刻では報告しない
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "gh-wait-review",
        &[URL, "--since", "2026-09-08T02:00:00Z"],
        &[("", 0), ("{\"number\":42}", 0), (REVIEWS, 0)],
        NO_WAIT,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert_eq!(
        stdout(&out),
        "INFO: 2026-09-08T02:00:00Z より新しいレビューの到着を待ちます\n\
         OK: PR #42 に新しいレビューが到着しました（2026-09-08T03:00:00Z、待機0秒）\n\
         - copilot-pull-request-reviewer: COMMENTED\n- null: COMMENTED\n- me: COMMENTED\n"
    );
    assert!(
        fake.log().starts_with(
            "gh auth status\ngh pr view 42 --json number\ngh pr view 42 --json reviews\n"
        ),
        "{}",
        fake.log()
    );
    assert!(
        fake.env()
            .lines()
            .all(|l| l == "GH_REPO=octo/repo" || l == "GH_HOST=github.com")
    );
}

#[test]
fn without_since_it_waits_for_reviews_after_the_last_request() {
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "gh-wait-review",
        &[URL],
        &[
            ("", 0),
            ("{\"number\":42}", 0),
            ("2026-09-08T01:00:00Z\n2026-09-08T02:00:00Z\n", 0),
            (REVIEWS, 0),
        ],
        NO_WAIT,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).starts_with(
            "INFO: 最後のレビュー要求は 2026-09-08T02:00:00Z。それ以降のレビューの到着を待ちます\n"
        ),
        "{}",
        stdout(&out)
    );
    assert!(
        fake.log().contains(
            "gh api --paginate repos/{owner}/{repo}/issues/42/timeline?per_page=100 --jq "
        )
    );
}

#[test]
fn no_review_after_since_times_out() {
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "gh-wait-review",
        &[URL, "--since", "2026-09-08T06:00:00Z"],
        &[
            ("", 0),
            ("{\"number\":42}", 0),
            (REVIEWS, 0),
            ("abc\n", 0),
            ("", 0),
            (REVIEWS, 0),
        ],
        NO_WAIT,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("TIMEOUT: "), "{}", stdout(&out));
    assert!(
        stdout(&out).contains(&format!("/gh-actions-check {URL} ")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_failed_copilot_run_on_the_head_stops_the_wait() {
    // トークン切れや内部エラーでレビューが走らなかったなら、待っても来ない
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "gh-wait-review",
        &[URL, "--since", "2026-09-08T06:00:00Z"],
        &[
            ("", 0),
            ("{\"number\":42}", 0),
            (REVIEWS, 0),
            ("abc\n", 0),
            ("Copilot\n", 0),
        ],
        NO_WAIT,
    );
    assert_eq!(out.status.code(), Some(7), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("レビューの実行が失敗"),
        "{}",
        stdout(&out)
    );
    assert!(
        fake.log().contains(
            "gh api --paginate repos/{owner}/{repo}/commits/abc/check-runs?per_page=100 --jq "
        ),
        "{}",
        fake.log()
    );
}

#[test]
fn no_request_stops_without_waiting() {
    // 要求していないレビューは来ない
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "gh-wait-review",
        &[URL],
        &[("", 0), ("{\"number\":42}", 0), ("", 0)],
        NO_WAIT,
    );
    assert_eq!(out.status.code(), Some(6), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("Copilot へのレビュー要求がありません"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains(&format!("request-rereview.sh {URL} ")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_failed_query_of_the_requests_stops() {
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "gh-wait-review",
        &[URL],
        &[("", 0), ("{\"number\":42}", 0), ("", 1)],
        NO_WAIT,
    );
    assert_eq!(out.status.code(), Some(6));
    assert!(
        stdout(&out).contains("レビュー要求を取得できません"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn gh_that_is_not_logged_in_or_a_missing_pr_stops_before_waiting() {
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env("gh-wait-review", &[URL], &[("", 1)], NO_WAIT);
    assert_eq!(out.status.code(), Some(4));
    assert!(stdout(&out).contains("未認証"), "{}", stdout(&out));
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env("gh-wait-review", &[URL], &[("", 0), ("", 1)], NO_WAIT);
    assert_eq!(out.status.code(), Some(3));
    assert!(
        stdout(&out).contains("PR #42 が見つからない"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn bad_arguments_stop_with_5() {
    for args in [
        &[][..],
        &["1"][..],
        &[URL, "--since"][..],
        &[URL, "--since", ""][..],
        &[URL, "--force"][..],
        &[URL, URL][..],
        &["https://github.com/octo/repo/pull/42#r1"][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls_with_env("gh-wait-review", args, &[("", 0)], NO_WAIT);
        assert_eq!(out.status.code(), Some(5), "{args:?}");
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
