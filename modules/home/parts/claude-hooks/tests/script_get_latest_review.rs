//! `claude-hooks script get-latest-review <PR の URL>` が、PR の最新の Copilot のレビューの要約（周回の材料、失敗の別、
//! 残る指摘の重大度、Suppressed comments と Previously missed の本文）をテキストで出すことを確かめる（ADR-0087。
//! `~/.claude/skills/gh-pr-review/scripts/get-latest-review.sh` が呼ぶ）。本文の形の細かい場合分けは、sh から呼ぶ
//! `.claude/tests/scripts/gh-pr-review.sh` も確かめる

mod common;

use common::fake_gh::{FakeGh, stderr};
use serde_json::json;

const URL: &str = "https://github.com/octo/repo/pull/42";

/// gh の `--paginate --jq '.[] | select(.user.login == "copilot-pull-request-reviewer[bot]")'` の出力
fn reviews(bodies: &[&str]) -> String {
    bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            format!(
                "{:#}\n",
                json!({"id": 8 + i, "state": "COMMENTED", "body": body})
            )
        })
        .collect()
}

fn sev(level: &str) -> String {
    format!(r#"<picture><img src="x.png" alt="{level} severity" width="12" height="12"></picture>"#)
}

/// 2026-09-19 からの本文（<!-- ccr-overview-v2 -->）。open は開いている指摘の行、missed は Previously missed の見出し
fn v2_body(headline: &str, open: &str, missed: Option<&str>) -> String {
    let mut body = format!(
        "<!-- ccr-overview-v2 -->\n\n### {headline}\n\nsummary\n\n<details open>\n\
         <summary><strong>open findings</strong></summary>\n\n{open}\n</details>\n"
    );
    if let Some(missed) = missed {
        body.push_str(&format!(
            "\n<details>\n<summary><strong>Previously missed (1)</strong></summary>\n\n\
             In code that hasn't changed since last review\n\n<details>\n<summary>{missed}</summary>\n\n\
             The detail of the missed finding.\n</details>\n</details>\n"
        ));
    }
    body.push_str("\n🧠 **Review effort:** Lite\n");
    body
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn no_copilot_review_is_round_0() {
    let fake = FakeGh::new();
    let out = fake.run_calls("get-latest-review", &[URL], &[("", 0)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "ROUND: 0\nNOTE: Copilot のレビューはまだありません\n"
    );
    assert_eq!(fake.env(), "GH_REPO=octo/repo\nGH_HOST=github.com\n");
}

#[test]
fn reads_the_reviews_and_the_inline_comments_of_the_latest() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "get-latest-review",
        &[URL],
        &[
            (
                &reviews(&["### 🟢 Approval recommended", "### 🟡 Changes recommended"]),
                0,
            ),
            ("101\n102\n", 0),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        fake.log(),
        "gh api --paginate repos/{owner}/{repo}/pulls/42/reviews?per_page=100 --jq .[] | \
         select(.user.login == \"copilot-pull-request-reviewer[bot]\")\n\
         gh api --paginate repos/{owner}/{repo}/pulls/42/reviews/9/comments?per_page=100 --jq .[].id\n"
    );
    assert_eq!(
        stdout(&out),
        "ROUND: 2\nREVIEW_ID: 9\nSTATE: COMMENTED\nHEADLINE: ### 🟡 Changes recommended\n\
         REVIEW_FAILED: no\nSUCCESSFUL_REVIEWS: 2\nINLINE_COMMENTS: 2\nSUPPRESSED_COMMENTS: 0\n\
         SEVERITY: unknown\n"
    );
}

#[test]
fn a_review_that_could_not_review_is_failed() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "get-latest-review",
        &[URL],
        &[
            (
                &reviews(&[
                    "### 🟡 Changes recommended",
                    "Copilot wasn't able to review any files in this pull request.",
                ]),
                0,
            ),
            ("", 0),
        ],
    );
    assert!(
        stdout(&out).contains("REVIEW_FAILED: yes\nSUCCESSFUL_REVIEWS: 1\nINLINE_COMMENTS: 0\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_v2_body_counts_the_open_findings_by_severity() {
    let body = v2_body(
        "🟡 Changes recommended",
        &format!(
            "- {} [m](u#discussion_r1) · New\n- {} [l](u#discussion_r2) · New\n- {} [h](u#discussion_r3)",
            sev("Medium"),
            sev("Low"),
            sev("High")
        ),
        None,
    );
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "get-latest-review",
        &[URL],
        &[(&reviews(&[&body]), 0), ("", 0)],
    );
    assert!(
        stdout(&out).ends_with(
            "SEVERITY: v2\nREMAINING_HIGH: 1\nREMAINING_MEDIUM: 1\nREMAINING_LOW: 1\nPREVIOUSLY_MISSED: 0\n"
        ),
        "{}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("HEADLINE: <!-- ccr-overview-v2 -->\n"));
}

#[test]
fn previously_missed_is_counted_and_shown() {
    let body = v2_body(
        "🔵 Needs a closer look",
        "",
        Some(&format!("{} Missed high", sev("High"))),
    );
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "get-latest-review",
        &[URL],
        &[(&reviews(&[&body]), 0), ("", 0)],
    );
    let text = stdout(&out);
    assert!(text.contains("REMAINING_HIGH: 1\n"), "{text}");
    assert!(
        text.contains("PREVIOUSLY_MISSED: 1\n--- previously missed ---\n"),
        "{text}"
    );
    assert!(
        text.ends_with("The detail of the missed finding.\n</details>\n</details>\n"),
        "{text}"
    );
}

#[test]
fn a_v2_body_whose_form_is_not_known_has_an_unknown_severity() {
    for body in [
        // 判定の見出しが無い（切れた本文、完走しなかったレビュー）
        v2_body("summary only", &format!("- {} [h](u)", sev("Low")), None),
        // アイコンの無い指摘
        v2_body("🟡 Changes recommended", "- [h](u)", None),
        // 知らない見出しの節
        v2_body("🟡 Changes recommended", "", None).replace("open findings", "new kind"),
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls(
            "get-latest-review",
            &[URL],
            &[(&reviews(&[&body]), 0), ("", 0)],
        );
        let text = stdout(&out);
        assert!(
            text.contains("SEVERITY: unknown\nPREVIOUSLY_MISSED: 0\n"),
            "{body}\n---\n{text}"
        );
        assert!(!text.contains("REMAINING_"), "{text}");
    }
}

#[test]
fn suppressed_comments_are_counted_and_shown() {
    let body = "### 🟢 Approval recommended\n\n<details>\n<summary>Suppressed comments (2)</summary>\n\n\
                - a.rs: maybe\n- b.rs: perhaps\n</details>\n- **Files reviewed:** 3\n";
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "get-latest-review",
        &[URL],
        &[(&reviews(&[body]), 0), ("", 0)],
    );
    let text = stdout(&out);
    assert!(text.contains("SUPPRESSED_COMMENTS: 2\n"), "{text}");
    assert!(
        text.ends_with(
            "--- suppressed ---\n<summary>Suppressed comments (2)</summary>\n\n- a.rs: maybe\n- b.rs: perhaps\n"
        ),
        "{text}"
    );
}

#[test]
fn the_exit_code_of_gh_is_the_exit_code_and_nothing_is_printed() {
    for calls in [
        &[("", 4)][..],
        &[(&reviews(&["### 🟢 Approval recommended"])[..], 0), ("", 5)][..],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls("get-latest-review", &[URL], calls);
        assert_eq!(out.status.code(), Some(calls.last().unwrap().1));
        assert_eq!(stdout(&out), "");
    }
}

#[test]
fn output_of_gh_that_is_not_reviews_is_an_error() {
    for gh_out in ["not json", "[1]", "{\"id\":\"x\"}"] {
        let fake = FakeGh::new();
        let out = fake.run_calls("get-latest-review", &[URL], &[(gh_out, 0), ("", 0)]);
        assert_eq!(out.status.code(), Some(1), "{gh_out}");
        assert_eq!(stdout(&out), "", "{gh_out}");
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
        let out = fake.run_calls("get-latest-review", args, &[("", 0)]);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&out).contains("Usage: get-latest-review.sh"),
            "{args:?}"
        );
        assert_eq!(fake.log(), "", "{args:?}");
    }
}
