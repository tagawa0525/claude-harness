#!/usr/bin/env bash
# .claude/scripts/gh-actions-diagnose.sh のテスト
#
# 実行: bash .claude/tests/scripts/gh-actions-diagnose.sh（まとめて実行: bash .claude/tests/scripts.sh）
#
# 実物の gh・ruff 等は使わず、make_fake_tool で PATH 先頭に置いた偽コマンドで
# 「スクリプトが何を呼び、出力をどう判定するか」を検証する。

source "$(dirname "$0")/../lib.sh"

# ===========================================================================
# gh-actions-diagnose.sh
# ===========================================================================

REPO="$TEST_ROOT/actions/app"
make_repo "$REPO"
git -C "$REPO" switch -q -c feat/x
cd "$REPO" || exit 1

RUN_FAIL='[{"databaseId":100,"name":"CI","status":"completed","conclusion":"failure","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/100"}]'
JOBS_FAIL='{"attempt":1,"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"Setup","conclusion":"success"},{"name":"Test","conclusion":"failure"}]}]}'

it "gh-actions-diagnose: run がなければ CAUSE: NONE と未設定の旨を出す"
make_fake_gh '"run list --branch feat/x"*) echo "[]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_eq 0 $?
assert_contains "$out" "CAUSE: NONE"
assert_contains "$out" "未設定"

it "gh-actions-diagnose: head SHA の check-run を取得できなければ unavailable と出し、Actions の判定はそのまま"
assert_contains "$out" "EXTERNAL_CHECKS: unavailable"

it "gh-actions-diagnose: 実行中なら CAUSE: IN_PROGRESS"
make_fake_gh '"run list --branch feat/x"*) echo "[{\"databaseId\":1,\"name\":\"CI\",\"status\":\"in_progress\",\"conclusion\":\"\",\"createdAt\":\"2026-09-01T00:00:00Z\",\"url\":\"u\"}]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: IN_PROGRESS"

it "gh-actions-diagnose: 成功なら CAUSE: NONE"
make_fake_gh '"run list --branch feat/x"*) echo "[{\"databaseId\":2,\"name\":\"CI\",\"status\":\"completed\",\"conclusion\":\"success\",\"createdAt\":\"2026-09-01T00:00:00Z\",\"url\":\"u\"}]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: NONE"

it "gh-actions-diagnose: 失敗ログに HTTP 5xx があれば TRANSIENT_API と再実行コマンドを出す"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_FAIL' ;;
  \"run view 100 --json jobs,attempt\"*) echo '$JOBS_FAIL' ;;
  \"run view 100 --log-failed\"*) printf 'build\tTest\t##[error]RequestError: HTTP 502 Bad Gateway\n' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_eq 0 $?
assert_contains "$out" "RUN: 100"
assert_contains "$out" "ATTEMPT: 1"
assert_contains "$out" "FAILED_JOB: build"
assert_contains "$out" "FAILED_STEP: Test"
assert_contains "$out" "CAUSE: TRANSIENT_API"
assert_contains "$out" "gh run rerun 100"

it "gh-actions-diagnose: Copilot の内部エラーなら COPILOT_INTERNAL"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_FAIL' ;;
  \"run view 100 --json jobs,attempt\"*) echo '$JOBS_FAIL' ;;
  \"run view 100 --log-failed\"*) printf 'review\tSetup\t##[error]Download ccrcli failed\n' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: COPILOT_INTERNAL"

it "gh-actions-diagnose: それ以外の失敗は CODE としてエラー行を出す"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_FAIL' ;;
  \"run view 100 --json jobs,attempt\"*) echo '$JOBS_FAIL' ;;
  \"run view 100 --log-failed\"*) printf 'build\tTest\t##[error]test_login failed: assertion error\n' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: CODE"
assert_contains "$out" "test_login failed"

it "gh-actions-diagnose: PR 番号を渡すと head ブランチを解決する"
make_fake_gh "\"pr view 7 --json headRefName,headRefOid\"*) echo '{\"headRefName\":\"feat/pr7\",\"headRefOid\":\"def\"}' ;;
  \"run list --branch feat/pr7\"*) echo '[]' ;;
  \"api --paginate repos/{owner}/{repo}/commits/def/check-runs\"*) : ;;
  \"api repos/{owner}/{repo}/commits/def/status\"*) echo '[]' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh" 7)
assert_contains "$out" "BRANCH: feat/pr7"
assert_contains "$out" "EXTERNAL_CHECKS: 0"
assert_not_contains "$(fake_log gh)" "git/ref"

it "gh-actions-diagnose: ブランチ名を渡すとそのブランチを見る"
make_fake_gh '"run list --branch other"*) echo "[]" ;;'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh" other)
assert_contains "$out" "BRANCH: other"

# --- 失敗の分類の追加: PERMISSION / EXTERNAL / 再実行回数 ---
# diagnose は GitHub Actions の run に加えて、head SHA の check-run（GitHub Actions 以外の App）と
# commit status（Buildkite 等の外部 CI）も見る。以下の偽 gh はその呼び出しをまとめて用意する

RUN_OK='[{"databaseId":100,"name":"CI","status":"completed","conclusion":"success","createdAt":"2026-09-01T00:00:00Z","url":"https://example/run/100"}]'
JOBS_FAIL_ATTEMPT2='{"attempt":2,"jobs":[{"name":"build","conclusion":"failure","steps":[{"name":"Test","conclusion":"failure"}]}]}'

# fake_gh_actions <runs_json> <jobs_json> <log_printf> [check_runs_lines] [status_json]
#   check_runs_lines は --jq 後の 1 行 1 JSON（GitHub Actions 以外の App のみ）、
#   status_json は commit status を check-run と同じ形に整えた配列
fake_gh_actions() {
  local runs="$1" jobs="$2" log="$3" checks="${4:-}" status="${5:-[]}"
  make_fake_gh "\"run list --branch feat/x\"*) echo '$runs' ;;
  \"run view 100 --json jobs,attempt\"*) echo '$jobs' ;;
  \"run view 100 --log-failed\"*) printf '$log' ;;
  \"api repos/{owner}/{repo}/git/ref/heads/feat/x\"*) echo abc ;;
  \"api --paginate repos/{owner}/{repo}/commits/abc/check-runs\"*) printf '%s\n' '$checks' ;;
  \"api repos/{owner}/{repo}/commits/abc/status\"*) echo '$status' ;;"
}

it "gh-actions-diagnose: 権限エラーは PERMISSION とし、再実行や default_workflow_permissions の緩和を提案しない"
fake_gh_actions "$RUN_FAIL" "$JOBS_FAIL" 'review\tPost\t##[error]HttpError: Resource not accessible by integration\n'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: PERMISSION"
assert_contains "$out" "permissions:"
assert_contains "$out" "default_workflow_permissions"
assert_not_contains "$out" "gh run rerun"

it "gh-actions-diagnose: 既に再実行済み（attempt 2 以上）の一時障害は再実行を提案しない"
fake_gh_actions "$RUN_FAIL" "$JOBS_FAIL_ATTEMPT2" 'build\tTest\t##[error]RequestError: HTTP 502 Bad Gateway\n'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: TRANSIENT_API"
assert_contains "$out" "ATTEMPT: 2"
assert_not_contains "$out" "gh run rerun 100"

it "gh-actions-diagnose: 外部 CI がなければ従来どおり NONE"
fake_gh_actions "$RUN_OK" '{}' ''
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "EXTERNAL_CHECKS: 0"
assert_contains "$out" "CAUSE: NONE"

it "gh-actions-diagnose: Actions が成功でも外部 CI（check-run の App）が失敗なら EXTERNAL とし details_url を示す"
fake_gh_actions "$RUN_OK" '{}' '' '{"name":"buildkite/app","status":"completed","conclusion":"failure","app":"buildkite","url":"https://buildkite.com/x/1"}'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: EXTERNAL"
assert_contains "$out" "EXTERNAL_CHECK: buildkite/app"
assert_contains "$out" "https://buildkite.com/x/1"
assert_not_contains "$out" "gh run rerun"

it "gh-actions-diagnose: commit status で報告される外部 CI の失敗も EXTERNAL"
fake_gh_actions "$RUN_OK" '{}' '' '' '[{"name":"ci/buildkite","status":"completed","conclusion":"failure","app":"commit-status","url":"https://buildkite.com/x/2"}]'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: EXTERNAL"
assert_contains "$out" "https://buildkite.com/x/2"

it "gh-actions-diagnose: 外部 CI が実行中なら IN_PROGRESS"
fake_gh_actions "$RUN_OK" '{}' '' '{"name":"buildkite/app","status":"in_progress","conclusion":null,"app":"buildkite","url":"https://buildkite.com/x/3"}'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: IN_PROGRESS"

it "gh-actions-diagnose: Actions の run がなくても外部 CI の失敗は EXTERNAL"
fake_gh_actions '[]' '{}' '' '{"name":"buildkite/app","status":"completed","conclusion":"failure","app":"buildkite","url":"https://buildkite.com/x/4"}'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "RUNS: 0"
assert_contains "$out" "CAUSE: EXTERNAL"

it "gh-actions-diagnose: head SHA はあっても check-run API が失敗したら unavailable（外部 CI なしと誤認しない）"
make_fake_gh "\"run list --branch feat/x\"*) echo '$RUN_OK' ;;
  \"api repos/{owner}/{repo}/git/ref/heads/feat/x\"*) echo abc ;;
  \"api --paginate repos/{owner}/{repo}/commits/abc/check-runs\"*) echo 'HTTP 502' >&2; exit 1 ;;
  \"api repos/{owner}/{repo}/commits/abc/status\"*) echo '[]' ;;"
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "EXTERNAL_CHECKS: unavailable"
assert_not_contains "$out" "EXTERNAL_CHECKS: 0"

it "gh-actions-diagnose: Actions が失敗なら外部 CI の結果に関わらず Actions 側の原因を出す"
fake_gh_actions "$RUN_FAIL" "$JOBS_FAIL" 'build\tTest\t##[error]test_login failed\n' '{"name":"buildkite/app","status":"completed","conclusion":"failure","app":"buildkite","url":"https://buildkite.com/x/5"}'
out=$("$SCRIPTS_DIR/gh-actions-diagnose.sh")
assert_contains "$out" "CAUSE: CODE"
assert_contains "$out" "EXTERNAL_CHECK: buildkite/app"

finish
