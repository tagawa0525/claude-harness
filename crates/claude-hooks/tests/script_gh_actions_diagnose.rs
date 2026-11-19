//! `claude-hooks script gh-actions-diagnose [PR の URL | PR 番号 | ブランチ名]` が、CI の最新の結果を調べて失敗の原因を
//! 分類することを確かめる（ADR-0087。`~/.claude/scripts/gh-actions-diagnose.sh` が呼ぶ）。原因の分類と外部の CI の
//! 場合分けは、sh から呼ぶ `claude/tests/scripts/gh-actions-diagnose.sh` も確かめる

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/7";
const PR: &str = r#"{"headRefName":"feat/y","headRefOid":"abc"}"#;
const RUN_FAIL: &str = r#"[{"databaseId":100,"name":"CI","workflowDatabaseId":1,"event":"pull_request","headSha":"abc","status":"completed","conclusion":"failure","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/100"}]"#;
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

#[test]
fn a_run_list_that_is_not_an_array_stops_instead_of_reporting_no_run() {
    for list in ["{}", "null", "\"oops\""] {
        let fake = FakeGh::new();
        let out = fake.run_calls("gh-actions-diagnose", &[URL], &[(PR, 0), (list, 0)]);
        assert_eq!(out.status.code(), Some(1), "{list}: {}", stdout(&out));
        assert!(!stdout(&out).contains("CAUSE:"), "{list}: {}", stdout(&out));
        assert!(stderr(&out).contains("配列"), "{list}: {}", stderr(&out));
    }
}

const RUN_OK: &str = r#"[{"databaseId":101,"name":"CI","workflowDatabaseId":1,"event":"pull_request","headSha":"abc","status":"completed","conclusion":"success","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/101"}]"#;
const RUN_GOING: &str = r#"[{"databaseId":102,"name":"CI","workflowDatabaseId":1,"event":"pull_request","headSha":"abc","status":"in_progress","conclusion":"","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/102"}]"#;

/// `--- report ---` から最後までの行
fn report(text: &str) -> &str {
    text.find("--- report ---\n")
        .map(|i| &text[i..])
        .unwrap_or_else(|| panic!("--- report --- が無い: {text}"))
}

#[test]
fn no_run_tells_what_to_report_on_the_next_line() {
    let fake = FakeGh::new();
    let out = fake.run_calls("gh-actions-diagnose", &["feat/x"], &[("[]", 0), ("", 1)]);
    let text = stdout(&out);
    assert!(
        text.contains(
            "CAUSE: NONE\nNOTE: ワークフローが未設定か、このブランチではまだ実行されていません\n\
             NEXT: ワークフローが未設定か未実行で、外部 CI は確認できていないと報告してください\n"
        ),
        "{text}"
    );
}

#[test]
fn a_successful_run_tells_to_report_success_and_whether_external_ci_was_seen() {
    // run の一覧、head の SHA、check-run、commit status
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(RUN_OK, 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    assert!(
        text.contains("CAUSE: NONE\nNEXT: head の SHA（abc）の GitHub Actions の run は、ワークフローごとの最新がすべて success です。そう報告してください\n"),
        "{text}"
    );

    // 外部の CI を読めなければ、その旨を添える（成功と言い切らない）
    let fake = FakeGh::new();
    let out = fake.run_calls("gh-actions-diagnose", &["feat/x"], &[(RUN_OK, 0), ("", 1)]);
    let text = stdout(&out);
    assert!(
        text.contains(
            "CAUSE: NONE\nNEXT: head の SHA（abc）の GitHub Actions の run は、ワークフローごとの最新がすべて success です。そう報告してください。外部 CI は確認できていないと添えてください\n"
        ),
        "{text}"
    );
}

#[test]
fn the_report_lines_gather_the_run_the_failures_the_external_ci_and_the_next_step() {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &[URL],
        &[
            (PR, 0),
            (RUN_FAIL, 0),
            (JOBS_FAIL, 0),
            ("##[error]Request failed: HTTP 502\n", 0),
            (
                r#"{"name":"lint","status":"completed","conclusion":"success","app":"circleci","url":"https://ci/1"}"#,
                0,
            ),
            ("[]", 0),
        ],
    );
    let text = stdout(&out);
    assert_eq!(
        report(&text),
        "--- report ---\nGitHub Actions 状況:\n- Run: 100（attempt 1）\n- Status: completed/failure\n\
         - 失敗ジョブ: build\n- 失敗ステップ: Test\n- 外部 CI: lint completed/success circleci https://ci/1\n\
         - 原因: TRANSIENT_API\n\
         - 推奨対応: GitHub API 側の一時障害です。1 回だけ再実行してください: gh run rerun 100 -R octo/repo\n"
    );
}

#[test]
fn the_report_lines_say_none_for_what_was_not_found() {
    // run が無く、外部の CI も読めない
    let fake = FakeGh::new();
    let out = fake.run_calls("gh-actions-diagnose", &["feat/x"], &[("[]", 0), ("", 1)]);
    assert_eq!(
        report(&stdout(&out)),
        "--- report ---\nGitHub Actions 状況:\n- Run: なし\n- Status: なし\n- 失敗ジョブ: なし\n\
         - 失敗ステップ: なし\n- 外部 CI: 確認できず\n- 原因: NONE\n\
         - 推奨対応: ワークフローが未設定か未実行で、外部 CI は確認できていないと報告してください\n"
    );

    // 実行中の run は attempt を読まないので、Run に attempt を添えない
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(RUN_GOING, 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    let rep = report(&text);
    assert!(
        rep.contains("- Run: 102\n- Status: in_progress/\n"),
        "{rep}"
    );
    assert!(
        rep.contains("- 外部 CI: なし\n- 原因: IN_PROGRESS\n"),
        "{rep}"
    );
}

#[test]
fn no_run_with_only_external_ci_tells_whether_it_passed() {
    // run の一覧、head の SHA、check-run（外部の CI が成功）、commit status
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[
            ("[]", 0),
            ("abc\n", 0),
            (
                r#"{"name":"lint","status":"completed","conclusion":"success","app":"circleci","url":"https://ci/1"}"#,
                0,
            ),
            ("[]", 0),
        ],
    );
    let text = stdout(&out);
    assert!(
        text.contains(
            "NEXT: GitHub Actions の run は無く、外部 CI に失敗はありません。その 2 点を報告してください\n"
        ),
        "{text}"
    );

    // run も外部の CI も無い
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[("[]", 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    assert!(
        text.contains("NEXT: ワークフローが未設定か未実行であると報告してください\n"),
        "{text}"
    );
}

#[test]
fn a_skipped_latest_run_is_not_reported_as_a_success() {
    let skipped = RUN_OK.replace("success", "skipped");
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(&skipped, 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    assert!(
        text.contains(
            "CAUSE: NONE\nNEXT: head の SHA（abc）の GitHub Actions の run に失敗と実行中はありません。success でない run（CI: skipped）を添え、成功とは書かずに報告してください\n"
        ),
        "{text}"
    );
}

#[test]
fn a_skipped_latest_run_without_external_ci_says_it_was_not_checked() {
    let skipped = RUN_OK.replace("success", "skipped");
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(&skipped, 0), ("", 1)],
    );
    let text = stdout(&out);
    assert!(
        text.contains(
            "NEXT: head の SHA（abc）の GitHub Actions の run に失敗と実行中はありません。success でない run（CI: skipped）を添え、成功とは書かずに報告してください。外部 CI は確認できていないと添えてください\n"
        ),
        "{text}"
    );
}

/// gh run list の 1 件（新しい順に並べて配列にする）
fn run(
    id: u64,
    name: &str,
    workflow: u64,
    event: &str,
    sha: &str,
    status: &str,
    conclusion: &str,
) -> String {
    format!(
        r#"{{"databaseId":{id},"name":"{name}","workflowDatabaseId":{workflow},"event":"{event}","headSha":"{sha}","status":"{status}","conclusion":"{conclusion}","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/{id}"}}"#
    )
}

fn runs(list: &[String]) -> String {
    format!("[{}]", list.join(","))
}

/// ブランチを渡し、run の一覧、head の SHA（abc）、失敗した run のジョブとログ、check-run、commit status の順に答える
fn diagnose_branch_with_a_failure(list: &str) -> String {
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[
            (list, 0),
            ("abc\n", 0),
            (JOBS_FAIL, 0),
            ("##[error]test_login failed\n", 0),
            ("", 0),
            ("[]", 0),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    stdout(&out)
}

#[test]
fn a_failed_workflow_at_the_head_is_diagnosed_even_when_another_workflow_finished_later() {
    let list = runs(&[
        run(
            201,
            "Lint",
            2,
            "pull_request",
            "abc",
            "completed",
            "success",
        ),
        run(100, "CI", 1, "pull_request", "abc", "completed", "failure"),
    ]);
    let text = diagnose_branch_with_a_failure(&list);
    assert!(
        text.contains("RUN: 100\nSTATUS: completed/failure\n"),
        "{text}"
    );
    assert!(text.contains("CAUSE: CODE\n"), "{text}");
    assert!(text.contains("gh run view 100 --log-failed"), "{text}");
}

#[test]
fn a_running_workflow_at_the_head_is_in_progress_even_when_another_workflow_succeeded() {
    let list = runs(&[
        run(
            201,
            "Lint",
            2,
            "pull_request",
            "abc",
            "completed",
            "success",
        ),
        run(202, "CI", 1, "pull_request", "abc", "in_progress", ""),
    ]);
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(&list, 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    assert!(text.contains("RUN: 202\nSTATUS: in_progress/\n"), "{text}");
    assert!(text.contains("CAUSE: IN_PROGRESS\n"), "{text}");
}

#[test]
fn only_the_latest_run_of_each_workflow_and_event_at_the_head_counts() {
    // 300 は同じワークフローと event の新しい run（301）に置き換わり、299 は head でない commit の run
    let list = runs(&[
        run(301, "CI", 1, "pull_request", "abc", "completed", "success"),
        run(300, "CI", 1, "pull_request", "abc", "completed", "failure"),
        run(
            299,
            "Lint",
            2,
            "pull_request",
            "old",
            "completed",
            "failure",
        ),
    ]);
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(&list, 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    assert!(
        text.contains(
            "RUN: 301\nSTATUS: completed/success\nEXTERNAL_CHECKS: 0\nCAUSE: NONE\n\
             NEXT: head の SHA（abc）の GitHub Actions の run は、ワークフローごとの最新がすべて success です。そう報告してください\n"
        ),
        "{text}"
    );
}

#[test]
fn runs_of_one_workflow_for_push_and_for_pull_request_are_judged_separately() {
    let list = runs(&[
        run(401, "CI", 1, "pull_request", "abc", "completed", "success"),
        run(100, "CI", 1, "push", "abc", "completed", "failure"),
    ]);
    let text = diagnose_branch_with_a_failure(&list);
    assert!(text.contains("RUN: 100\n"), "{text}");
    assert!(text.contains("CAUSE: CODE\n"), "{text}");
}

#[test]
fn the_next_step_names_the_other_workflows_that_failed_or_are_running() {
    let list = runs(&[
        run(
            100,
            "Lint",
            2,
            "pull_request",
            "abc",
            "completed",
            "failure",
        ),
        run(500, "CI", 1, "pull_request", "abc", "completed", "failure"),
        run(501, "Docs", 3, "pull_request", "abc", "queued", ""),
        run(
            502,
            "Build",
            4,
            "pull_request",
            "abc",
            "completed",
            "success",
        ),
    ]);
    let text = diagnose_branch_with_a_failure(&list);
    assert!(text.contains("RUN: 100\n"), "{text}");
    assert!(
        text.contains(
            "。ほかのワークフローにも失敗か実行中の run があります: CI 500 completed/failure, Docs 501 queued/\n"
        ),
        "{text}"
    );
}

#[test]
fn runs_of_older_commits_do_not_decide_the_cause_of_a_head_without_runs() {
    let list = runs(&[run(
        600,
        "CI",
        1,
        "pull_request",
        "old",
        "completed",
        "failure",
    )]);
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[(&list, 0), ("abc\n", 0), ("", 0), ("[]", 0)],
    );
    let text = stdout(&out);
    assert!(!text.contains("RUN: "), "{text}");
    assert!(
        text.contains(
            "CAUSE: NONE\nNEXT: head の SHA（abc）の GitHub Actions の run はまだありません（RUNS はそれより前の commit の run）。起動を待って再確認し、それでも無ければ、この commit ではワークフローが走っていないと報告してください\n"
        ),
        "{text}"
    );
}

#[test]
fn without_the_head_of_the_branch_the_commit_of_the_latest_run_is_judged() {
    // git/ref を読めない（未 push か API の失敗）。外部の CI は読まずに確認できずとする
    let list = runs(&[
        run(
            201,
            "Lint",
            2,
            "pull_request",
            "abc",
            "completed",
            "success",
        ),
        run(100, "CI", 1, "pull_request", "abc", "completed", "failure"),
    ]);
    let fake = FakeGh::new();
    let out = fake.run_calls(
        "gh-actions-diagnose",
        &["feat/x"],
        &[
            (&list, 0),
            ("", 1),
            (JOBS_FAIL, 0),
            ("##[error]test_login failed\n", 0),
        ],
    );
    let text = stdout(&out);
    assert!(text.contains("RUN: 100\n"), "{text}");
    assert!(text.contains("EXTERNAL_CHECKS: unavailable\n"), "{text}");
    assert!(text.contains("CAUSE: CODE\n"), "{text}");
}
