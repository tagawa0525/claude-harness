//! `claude-hooks script gh-wait-checks <PR の URL>` が、PR の head の check（check run とコミットの status）が登録されて
//! 全部が終わるまで待ち、通ったか落ちたかを出すことを確かめる（`~/.claude/scripts/gh-wait-checks.sh` が呼ぶ）。push の
//! 直後は check がまだ登録されておらず、`gh pr checks --watch` はそこで終わってしまう

mod common;

use common::fake_gh::{FakeGh, stderr};

const URL: &str = "https://github.com/octo/repo/pull/42";
const HEAD: &str = "def5678def5678def5678def5678def5678def56\n";
/// 待つ間隔を 0 にする（既定は約 20 分と、登録を待つ約 1 分）
const NO_WAIT: &[(&str, &str)] = &[
    ("GH_WAIT_CHECKS_INTERVALS", "0 0 0"),
    ("GH_WAIT_CHECKS_REGISTER_INTERVALS", "0 0"),
    ("GH_WAIT_CHECKS_SETTLE", "0"),
];
const PASSED: &str = "{\"name\":\"cargo-test\",\"status\":\"completed\",\"conclusion\":\"success\"}\n\
                      {\"name\":\"setup\",\"status\":\"completed\",\"conclusion\":\"skipped\"}\n";
const RUNNING: &str = "{\"name\":\"cargo-test\",\"status\":\"in_progress\",\"conclusion\":null}\n\
                       {\"name\":\"setup\",\"status\":\"queued\",\"conclusion\":null}\n";
const FAILED: &str = "{\"name\":\"cargo-test\",\"status\":\"completed\",\"conclusion\":\"failure\"}\n\
                      {\"name\":\"setup\",\"status\":\"completed\",\"conclusion\":\"success\"}\n";

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 起動の確認（gh auth status、PR の有無）の後に、rounds の順で head、check run、status を返す
fn wait(fake: &FakeGh, rounds: &[(&str, &str)]) -> std::process::Output {
    let mut calls = vec![("", 0), ("{\"number\":42}", 0)];
    for (runs, statuses) in rounds {
        calls.extend([(HEAD, 0), (*runs, 0), (*statuses, 0)]);
    }
    fake.run_calls_with_env("gh-wait-checks", &[URL], &calls, NO_WAIT)
}

#[test]
fn finished_checks_that_passed_end_with_passed() {
    let fake = FakeGh::new();
    let out = wait(&fake, &[(PASSED, "[]"); 2]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert_eq!(
        stdout(&out),
        "HEAD: def5678def5678def5678def5678def5678def56\nCHECKS: passed（2 件、待機 0 秒）\n"
    );
    assert!(
        fake.log().starts_with(
            "gh auth status\ngh pr view 42 --json number\ngh pr view 42 --json headRefOid -q .headRefOid\n\
             gh api --paginate repos/{owner}/{repo}/commits/def5678def5678def5678def5678def5678def56/check-runs?per_page=100"
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
fn it_waits_while_a_check_is_running() {
    let fake = FakeGh::new();
    let out = wait(&fake, &[(RUNNING, "[]"), (PASSED, "[]"), (PASSED, "[]")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("待機中: 実行中の check が 2 件（cargo-test, setup）"),
        "{}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("CHECKS: passed"), "{}", stdout(&out));
}

#[test]
fn it_waits_for_checks_to_be_registered_after_a_push() {
    // push の直後は check が 1 件も無い。通ったと数えず、登録を待つ
    let fake = FakeGh::new();
    let out = wait(
        &fake,
        &[("", "[]"), (RUNNING, "[]"), (PASSED, "[]"), (PASSED, "[]")],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("待機中: check がまだ登録されていません"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("CHECKS: passed（2 件"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_head_that_never_gets_a_check_ends_with_none() {
    // CI の無いリポジトリでは、登録を待つ間（約 1 分）で切り上げる
    let fake = FakeGh::new();
    let out = wait(&fake, &[("", "[]"), ("", "[]"), ("", "[]")]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(stdout(&out).ends_with("CHECKS: none\n"), "{}", stdout(&out));
}

#[test]
fn a_failed_check_ends_with_failed_and_its_name() {
    let fake = FakeGh::new();
    let out = wait(&fake, &[(FAILED, "[]")]);
    assert_eq!(
        out.status.code(),
        Some(6),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).ends_with("CHECKS: failed\nFAILED: cargo-test (failure)\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_commit_status_counts_as_a_check() {
    // 外部の CI はコミットの status で知らせる
    let fake = FakeGh::new();
    let pending = "[{\"name\":\"ci/ext\",\"status\":\"in_progress\",\"conclusion\":\"pending\"}]";
    let failed = "[{\"name\":\"ci/ext\",\"status\":\"completed\",\"conclusion\":\"failure\"}]";
    let out = wait(&fake, &[("", pending), ("", failed)]);
    assert_eq!(
        out.status.code(),
        Some(6),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("FAILED: ci/ext (failure)"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_failed_check_is_reported_without_waiting_for_the_running_ones() {
    // 落ちた check があれば、ほかの check の終わり（や時間切れ）を待たずに知らせる
    let fake = FakeGh::new();
    let runs = "{\"name\":\"cargo-test\",\"status\":\"completed\",\"conclusion\":\"failure\"}\n\
                {\"name\":\"windows\",\"status\":\"in_progress\",\"conclusion\":null}\n";
    let out = wait(&fake, &[(runs, "[]")]);
    assert_eq!(
        out.status.code(),
        Some(6),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).ends_with("CHECKS: failed\nFAILED: cargo-test (failure)\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn checks_that_appear_after_the_first_ones_passed_are_waited_for() {
    // 先に登録された check（すぐ skipped になる job、外部の status）だけが終わった時点で、通ったとしない。少し置いて
    // もう 1 度見て、同じなら通ったとする
    let fake = FakeGh::new();
    let first = "{\"name\":\"setup\",\"status\":\"completed\",\"conclusion\":\"skipped\"}\n";
    let out = wait(
        &fake,
        &[
            (first, "[]"),
            (RUNNING, "[]"),
            (PASSED, "[]"),
            (PASSED, "[]"),
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("待機中: 実行中の check が 2 件"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).ends_with("CHECKS: passed（2 件、待機 0 秒）\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn with_a_sha_it_waits_until_the_pr_head_is_that_commit() {
    // push の直後は、PR の head がまだ前のコミットのことがある。前の head の通った check を、今の push の結果にしない
    const OLD: &str = "abc1234abc1234abc1234abc1234abc1234abc12\n";
    let fake = FakeGh::new();
    let mut calls = vec![("", 0), ("{\"number\":42}", 0)];
    calls.extend([(OLD, 0)]);
    for _ in 0..2 {
        calls.extend([(HEAD, 0), (PASSED, 0), ("[]", 0)]);
    }
    let out = fake.run_calls_with_env("gh-wait-checks", &[URL, "def5678"], &calls, NO_WAIT);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("待機中: PR の head がまだ def5678 になっていません（今は abc1234）"),
        "{}",
        stdout(&out)
    );
    assert!(!stdout(&out).contains("HEAD: abc1234"), "{}", stdout(&out));
    assert!(stdout(&out).contains("CHECKS: passed"), "{}", stdout(&out));
}

#[test]
fn a_new_head_gets_its_own_time_to_register_checks() {
    // 待つ間に push された head にも、check の登録を待つ時間を初めから与える
    const NEW: &str = "abc1234abc1234abc1234abc1234abc1234abc12\n";
    let fake = FakeGh::new();
    let mut calls = vec![("", 0), ("{\"number\":42}", 0)];
    // 前の head: 登録を待つ間隔（2 つ）を使い切る手前まで check が無い
    for _ in 0..2 {
        calls.extend([(HEAD, 0), ("", 0), ("[]", 0)]);
    }
    // 新しい head: まだ check が無い → 登録される → 通る（確認の 1 回を含む）
    calls.extend([(NEW, 0), ("", 0), ("[]", 0)]);
    calls.extend([(NEW, 0), ("", 0), ("[]", 0)]);
    for _ in 0..2 {
        calls.extend([(NEW, 0), (PASSED, 0), ("[]", 0)]);
    }
    let out = fake.run_calls_with_env("gh-wait-checks", &[URL], &calls, NO_WAIT);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(stdout(&out).contains("CHECKS: passed"), "{}", stdout(&out));
}

#[test]
fn a_failed_copilot_review_run_is_not_a_failed_check() {
    // レビューの実行の失敗はコードの問題でない（マージの hook も CI の失敗に数えない）
    let fake = FakeGh::new();
    let runs = "{\"name\":\"Copilot code review\",\"status\":\"completed\",\"conclusion\":\"failure\"}\n\
                {\"name\":\"cargo-test\",\"status\":\"completed\",\"conclusion\":\"success\"}\n";
    let out = wait(&fake, &[(runs, "[]"); 2]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(stdout(&out).contains("CHECKS: passed"), "{}", stdout(&out));
}

#[test]
fn checks_still_running_at_the_end_are_a_timeout() {
    let fake = FakeGh::new();
    // 待つ間隔は 3 つなので、見るのは 4 回（偽の gh は、渡した応答が尽きると最後の応答を返し続ける）
    let out = wait(&fake, &[(RUNNING, "[]"); 4]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("TIMEOUT: 実行中の check が残っています（cargo-test, setup）"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn a_state_it_cannot_read_three_times_in_a_row_stops_with_the_way_to_see_why() {
    // check run か status を取れない回は、通ったとも無いとも数えない。続けて取れなければ、時間切れまで待たずに止まる
    // （権限の無いトークンなど、待っても変わらない失敗）
    let fake = FakeGh::new();
    let mut calls = vec![("", 0), ("{\"number\":42}", 0)];
    for _ in 0..3 {
        calls.extend([(HEAD, 0), ("", 1), ("[]", 0)]);
    }
    let env: Vec<(&str, &str)> = NO_WAIT
        .iter()
        .copied()
        .chain([("GH_WAIT_CHECKS_INTERVALS", "0 0 0 0 0 0")])
        .collect();
    let out = fake.run_calls_with_env("gh-wait-checks", &[URL], &calls, &env);
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("ERROR: check の状態を 3 回続けて取得できません"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("gh api repos/octo/repo/commits/"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn bad_arguments_and_a_missing_pr_stop_early() {
    let fake = FakeGh::new();
    for args in [
        &[][..],
        &["42"][..],
        &[URL, URL][..],
        &[URL, "--x"][..],
        &[URL, "def5678", "x"][..],
    ] {
        let out = fake.run_calls_with_env("gh-wait-checks", args, &[("", 0)], NO_WAIT);
        assert_eq!(out.status.code(), Some(5), "{args:?}");
    }
    let out = fake.run_calls_with_env("gh-wait-checks", &[URL], &[("", 0), ("", 1)], NO_WAIT);
    assert_eq!(out.status.code(), Some(3));
    let out = fake.run_calls_with_env("gh-wait-checks", &[URL], &[("", 1)], NO_WAIT);
    assert_eq!(out.status.code(), Some(4));
}
