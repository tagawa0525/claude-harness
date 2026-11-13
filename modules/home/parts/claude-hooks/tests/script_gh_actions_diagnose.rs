//! `claude-hooks script gh-actions-diagnose [PR の URL | PR 番号 | ブランチ名]` が、CI の最新の結果を調べて失敗の原因を
//! 分類することを確かめる（ADR-0087。`~/.claude/scripts/gh-actions-diagnose.sh` が呼ぶ）。原因の分類と外部の CI の
//! 場合分けは、sh から呼ぶ `.claude/tests/scripts/gh-actions-diagnose.sh` も確かめる

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/7";
const PR: &str = r#"{"headRefName":"feat/y","headRefOid":"abc"}"#;
const RUN_FAIL: &str = r#"[{"databaseId":100,"name":"CI","status":"completed","conclusion":"failure","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/100"}]"#;
const JOBS_FAIL: &str = r#"{"attempt":1,"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"Setup","conclusion":"success"},{"name":"Test","conclusion":"failure"}]}]}"#;

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_transient_api_failure_of_a_pr_url_suggests_one_rerun_in_that_repository() {
    // PR、run の一覧、ジョブ、失敗したログ、check-run、commit status
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &[URL],
        &[
            (PR, 0),
            (RUN_FAIL, 0),
            (JOBS_FAIL, 0),
            ("##[error]Request failed: HTTP 502\n", 0),
            ("", 0),
            ("[]", 0),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with(
            "BRANCH: feat/y\nPR: 7\nRUNS:\n- 100 CI completed/failure 2026-09-01T00:00:00Z https://example/run/100\n\
             RUN: 100\nSTATUS: completed/failure\nATTEMPT: 1\nFAILED_JOB: build\nFAILED_STEP: Test\n--- errors ---\n\
             ##[error]Request failed: HTTP 502\nEXTERNAL_CHECKS: 0\n"
        ),
        "{text}"
    );
    assert!(text.contains("CAUSE: TRANSIENT_API\n"), "{text}");
    assert!(text.contains("gh run rerun 100 -R octo/repo"), "{text}");
    assert!(
        fake.env()
            .lines()
            .all(|l| l == "GH_REPO=octo/repo" || l == "GH_HOST=github.com")
    );
}

#[test]
fn no_run_is_none_and_unreachable_checks_are_unavailable() {
    let fake = FakeGh::new();
    let out = fake.run_calls("gh-actions-diagnose", &["feat/x"], &[("[]", 0), ("", 1)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with("BRANCH: feat/x\nRUNS: 0\nEXTERNAL_CHECKS: unavailable\n"),
        "{text}"
    );
    assert!(text.contains("CAUSE: NONE\n"), "{text}");
    assert!(text.contains("未設定"), "{text}");
}

#[test]
fn a_url_that_is_not_a_pr_stops() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["https://github.com/octo/repo/issues/1"],
        &[("", 0)],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("PR の URL"), "{}", stderr(&out));
    assert_eq!(fake.log(), "");
}

#[test]
fn output_of_gh_that_is_not_json_stops_instead_of_reporting_a_healthy_ci() {
    for calls in [
        // PR の情報
        vec![("not json", 0)],
        // run の一覧
        vec![(PR, 0), ("warning: something\n[]", 0)],
        // ジョブ（attempt を 1 と取り違えて再実行を勧めない）
        vec![(PR, 0), (RUN_FAIL, 0), ("truncated {", 0)],
    ] {
        let fake = FakeGh::new();
        let out = fake.run_calls("gh-actions-diagnose", &[URL], &calls);
        assert_eq!(out.status.code(), Some(1), "{calls:?}: {}", stdout(&out));
        assert!(
            !stdout(&out).contains("CAUSE:"),
            "{calls:?}: {}",
            stdout(&out)
        );
        assert!(stderr(&out).contains("JSON"), "{calls:?}: {}", stderr(&out));
    }
}
