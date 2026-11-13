//! `claude-hooks script gh-review-requests <PR の URL>` が、PR の Copilot 宛てのレビューの要求の時刻を古い順に 1 行ずつ
//! 出すことを確かめる（ADR-0087。`~/.claude/scripts/gh-review-requests.sh` が呼ぶ）。「レビューを待つ」「何周目か」
//! 「再レビューが登録されたか」はどれもこの列から決まる

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";

#[test]
fn prints_the_times_of_the_requests_to_copilot() {
    let fake = FakeGh::new();
    let out = fake.run(
        "gh-review-requests",
        &[URL],
        "2026-09-08T00:00:00Z\n2026-09-08T02:00:00Z\n",
        0,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "2026-09-08T00:00:00Z\n2026-09-08T02:00:00Z\n"
    );
    // レビュアーのログインは API により Copilot と copilot-pull-request-reviewer[bot] に揺れるので、Bot の種別で
    // login に copilot を含むものを数える
    assert_eq!(
        fake.log(),
        "gh api --paginate repos/{owner}/{repo}/issues/42/timeline?per_page=100 --jq .[] | \
         select(.event == \"review_requested\"\n                     and (.requested_reviewer.type? // \"\") == \"Bot\"\n                     \
         and ((.requested_reviewer.login? // \"\") | ascii_downcase | test(\"copilot\")))\n           | .created_at\n"
    );
    assert_eq!(fake.env(), "GH_REPO=octo/repo\nGH_HOST=github.com\n");
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code() {
    let fake = FakeGh::new();
    let out = fake.run("gh-review-requests", &[URL], "", 4);
    assert_eq!(out.status.code(), Some(4));
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
        let out = fake.run("gh-review-requests", args, "", 0);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: gh-review-requests.sh"),
            "{args:?}"
        );
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
