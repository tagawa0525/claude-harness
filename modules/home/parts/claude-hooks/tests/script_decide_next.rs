//! `claude-hooks script decide-next <PR の URL>` が、PR のレビューの状態から次の一手（VERDICT）を 1 つに決めることを
//! 確かめる（ADR-0087。`~/.claude/skills/gh-pr-review/scripts/decide-next.sh` が呼ぶ）。判定の表の場合分け（周回の
//! 数え方、文書だけの PR、レビューの失敗、重大度、ローカルレビューの記録）は、sh から呼ぶ
//! `.claude/tests/scripts/gh-pr-review.sh` が確かめる。記録の判定は `claude-hooks local-review-covers` を呼ぶ
//! （CLAUDE_HOOKS_BIN があればそれ、無ければ自分。sh のテストが偽物に差し替える）

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";

#[test]
fn no_review_and_no_record_is_waiting() {
    // 要求の一覧、数え直しの印、レビューの一覧（Copilot のレビューなし）、head、未解決スレッド、記録（local-review-covers
    // の中の gh）
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "decide-next",
        &[URL],
        &[
            ("2026-09-08T00:00:00Z\n", 0),
            ("", 0),
            ("", 0),
            ("def5678def5678def5678def5678def5678def56\n", 0),
            ("", 0),
            ("", 0),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("ROUND: 1\nRESPONSE: none\n"), "{text}");
    assert!(
        text.contains("UNRESOLVED: 0\nLOCAL_REVIEWED: no\n"),
        "{text}"
    );
    assert!(text.ends_with("VERDICT: WAITING\n"), "{text}");
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
        let out = fake.run_calls_with_env("decide-next", args, &[("", 0)], &[]);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(stderr(&out).contains("Usage: decide-next.sh"), "{args:?}");
        assert_eq!(fake.log(), "", "{args:?}");
    }
}

#[test]
fn a_failed_query_of_the_latest_review_stops_without_a_verdict() {
    // 要求の一覧、印、レビューの一覧、インラインのコメント、レビューの詳細（失敗）、head、未解決スレッド、記録。
    // レビューが届いた後にその詳細を取れなければ、応答が無いとみなして WAITING などを出さない
    let reviews = "{\"id\":9,\"state\":\"COMMENTED\",\"body\":\"### 🟡 Changes recommended\"}";
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env(
        "decide-next",
        &[URL],
        &[
            ("2026-09-08T00:00:00Z\n", 0),
            ("", 0),
            (reviews, 0),
            ("", 0),
            ("", 1),
            ("def5678def5678def5678def5678def5678def56\n", 0),
            ("", 0),
            ("", 0),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("VERDICT"));
    assert!(stderr(&out).contains("ERROR: "), "{}", stderr(&out));
}
