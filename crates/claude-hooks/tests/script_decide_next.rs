//! `claude-hooks script decide-next <PR の URL>` が、PR のレビューの状態から次の一手（VERDICT）を 1 つに決めることを
//! 確かめる（ADR-0087。`~/.claude/skills/gh-pr-review/scripts/decide-next.sh` が呼ぶ）。判定の表の場合分け（周回の
//! 数え方、文書だけの PR、レビューの失敗、重大度、ローカルレビューの記録）は、sh から呼ぶ
//! `claude/tests/scripts/gh-pr-review.sh` が確かめる。記録の判定は `claude-hooks local-review-covers` を呼ぶ
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
    assert!(text.contains("\nVERDICT: WAITING\nNEXT: "), "{text}");
}

const HEAD: &str = "def5678def5678def5678def5678def5678def56";
const REVIEWED: &str = "abc1234abc1234abc1234abc1234abc1234abc12";
const OPEN_THREAD: &str = r#"{"id":"T1","isResolved":false,"isOutdated":false,"path":"a.txt","line":3,"comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":11,"id":"C1","body":"fix this","author":{"__typename":"Bot","login":"copilot-pull-request-reviewer"},"createdAt":"2026-09-08T02:30:00Z","url":"u1","replyTo":null}]}}"#;
const FAILED_BODY: &str =
    "Copilot encountered an error and was unable to review this pull request.";

/// Copilot のレビュー 1 件（本文、インラインの指摘の ID の行、提出の時刻、見たコミット）
struct Review<'a> {
    body: &'a str,
    inline: &'a str,
    at: &'a str,
    reviewed: &'a str,
}

/// 要求 2 件（00:00、02:00）の PR で decide-next を流した標準出力。gh の呼び出しの順は、要求の一覧、数え直しの印、
/// レビューの一覧、（レビューがあれば）インラインの指摘とレビューの詳細、head、未解決スレッド、記録（local-review-covers
/// の中の gh）、（呼ばれれば）変更ファイル
fn decide(review: Option<Review>, threads: &str, recorded: bool, files: &str) -> String {
    let record = if recorded {
        format!("<!-- local-review sha={HEAD} high=0 medium=0 low=0 -->\n")
    } else {
        String::new()
    };
    let (reviews, detail);
    let mut calls: Vec<(&str, i32)> =
        vec![("2026-09-08T00:00:00Z\n2026-09-08T02:00:00Z\n", 0), ("", 0)];
    match &review {
        Some(r) => {
            reviews =
                serde_json::json!({"id": 9, "state": "COMMENTED", "body": r.body}).to_string();
            detail = format!("{} {}\n", r.at, r.reviewed);
            calls.extend([(reviews.as_str(), 0), (r.inline, 0), (detail.as_str(), 0)]);
        }
        None => calls.push(("", 0)),
    }
    let head = format!("{HEAD}\n");
    calls.extend([
        (head.as_str(), 0),
        (threads, 0),
        (record.as_str(), 0),
        (files, 0),
    ]);
    let fake = FakeGh::new();
    let out = fake.run_calls_with_env("decide-next", &[URL], &calls, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// VERDICT の行と、その次の行
fn verdict_and_next(text: &str) -> (&str, &str) {
    let mut lines = text.lines().skip_while(|l| !l.starts_with("VERDICT: "));
    let verdict = lines
        .next()
        .unwrap_or_else(|| panic!("VERDICT が無い: {text}"));
    (verdict, lines.next().unwrap_or(""))
}

fn assert_next(text: &str, verdict: &str, parts: &[&str]) {
    let (got, next) = verdict_and_next(text);
    assert_eq!(got, format!("VERDICT: {verdict}"), "{text}");
    assert!(next.starts_with("NEXT: "), "{text}");
    for part in parts {
        assert!(next.contains(part), "{part} が NEXT に無い: {text}");
    }
}

#[test]
fn waiting_is_followed_by_recording_a_local_review() {
    let text = decide(None, "", false, "a.txt\n");
    assert_next(
        &text,
        "WAITING",
        &[
            &format!("record-local-review.sh {URL} "),
            &format!("gh-wait-review.sh {URL}"),
        ],
    );
}

#[test]
fn act_is_followed_by_steps_3_to_5() {
    let review = Review {
        body: "### 🟡 Changes recommended",
        inline: "101\n",
        at: "2026-09-08T03:00:00Z",
        reviewed: HEAD,
    };
    let text = decide(Some(review), OPEN_THREAD, false, "a.txt\n");
    assert_next(&text, "ACT", &["Step 3〜5"]);
}

#[test]
fn local_review_needed_is_followed_by_recording_and_running_decide_next_again() {
    let review = Review {
        body: "### 🟡 Changes recommended",
        inline: "101\n",
        at: "2026-09-08T03:00:00Z",
        reviewed: REVIEWED,
    };
    let text = decide(Some(review), "", false, "src/a.rs\n");
    assert_next(
        &text,
        "LOCAL_REVIEW_NEEDED",
        &[
            &format!("record-local-review.sh {URL} "),
            &format!("decide-next.sh {URL}"),
            "Step 7",
        ],
    );
}

#[test]
fn docs_reviewed_is_followed_by_step_7_and_a_local_review_before_pushing_later_fixes() {
    let review = Review {
        body: "### 🟡 Changes recommended",
        inline: "101\n",
        at: "2026-09-08T03:00:00Z",
        reviewed: REVIEWED,
    };
    let text = decide(Some(review), "", false, "docs/adr/0004-x.md\n");
    assert_next(
        &text,
        "STOP_DOCS_REVIEWED",
        &["Step 7", "/code-review low", "push"],
    );
}

#[test]
fn local_reviewed_with_copilot_findings_notes_the_ledger() {
    let review = Review {
        body: "### 🟡 Changes recommended",
        inline: "101\n102\n",
        at: "2026-09-08T03:00:00Z",
        reviewed: REVIEWED,
    };
    let text = decide(Some(review), "", true, "src/a.rs\n");
    assert_next(&text, "STOP_LOCAL_REVIEWED", &["Step 7"]);
    let note = text
        .lines()
        .find(|l| l.starts_with("NOTE: "))
        .unwrap_or_else(|| panic!("NOTE が無い: {text}"));
    assert!(note.contains("PREVIOUSLY_MISSED"), "{text}");
    assert!(note.contains("台帳"), "{text}");
}

#[test]
fn local_reviewed_before_the_requested_review_arrives_notes_the_ledger_of_the_earlier_review() {
    // 要求（02:00）の後のレビューは未着（RESPONSE: none）でも、前の周（01:00）の指摘は台帳に載っているはず
    let review = Review {
        body: "### 🟡 Changes recommended",
        inline: "101\n",
        at: "2026-09-08T01:00:00Z",
        reviewed: REVIEWED,
    };
    let text = decide(Some(review), "", true, "src/a.rs\n");
    assert!(text.contains("RESPONSE: none\n"), "{text}");
    assert_next(&text, "STOP_LOCAL_REVIEWED", &["Step 7"]);
    assert!(
        text.lines()
            .any(|l| l.starts_with("NOTE: ") && l.contains("台帳")),
        "{text}"
    );
}

#[test]
fn local_reviewed_without_copilot_findings_has_no_ledger_note() {
    let text = decide(None, "", true, "src/a.rs\n");
    assert_next(&text, "STOP_LOCAL_REVIEWED", &["Step 7"]);
    assert!(!text.contains("NOTE: "), "{text}");
}

#[test]
fn review_failed_is_followed_by_recording_a_local_review() {
    let review = Review {
        body: FAILED_BODY,
        inline: "",
        at: "2026-09-08T03:00:00Z",
        reviewed: HEAD,
    };
    let text = decide(Some(review), "", false, "src/a.rs\n");
    assert_next(
        &text,
        "REVIEW_FAILED",
        &[
            &format!("record-local-review.sh {URL} "),
            &format!("/gh-actions-check {URL}"),
        ],
    );
}

#[test]
fn declined_and_clean_are_followed_by_step_7() {
    let review = Review {
        body: "### 🟡 Changes recommended",
        inline: "101\n",
        at: "2026-09-08T03:00:00Z",
        reviewed: HEAD,
    };
    assert_next(
        &decide(Some(review), "", false, "a.txt\n"),
        "STOP_DECLINED",
        &["Step 7"],
    );
    let review = Review {
        body: "### 🟢 Approval recommended",
        inline: "",
        at: "2026-09-08T03:00:00Z",
        reviewed: HEAD,
    };
    assert_next(
        &decide(Some(review), "", false, "a.txt\n"),
        "STOP_CLEAN",
        &["Step 7"],
    );
}

#[test]
fn previously_missed_is_followed_by_step_3_and_then_the_findings() {
    let body = "<!-- ccr-overview-v2 -->\n\n### 🔵 Needs a closer look\n\nsummary\n\n<details open>\n\
                <summary><strong>open findings</strong></summary>\n\n\n</details>\n\n<details>\n\
                <summary><strong>Previously missed (1)</strong></summary>\n\n\
                In code that hasn't changed since last review\n\n<details>\n\
                <summary><img alt=\"Medium severity\"> Missed medium</summary>\n\n`src/a.rs:10`\n\n\
                The detail of the missed finding.\n</details>\n</details>\n\n🧠 **Review effort:** Lite\n";
    let review = Review {
        body,
        inline: "",
        at: "2026-09-08T03:00:00Z",
        reviewed: HEAD,
    };
    let text = decide(Some(review), "", false, "a.txt\n");
    assert_next(&text, "STOP_PREVIOUSLY_MISSED", &["Step 3"]);
    let after_next = text
        .split_once("\nNEXT: ")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    assert!(after_next.contains("--- previously missed ---"), "{text}");
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
