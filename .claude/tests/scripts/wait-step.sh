#!/usr/bin/env bash
# .claude/scripts/wait-step.sh（/delegate の担当が、ターンを終えずに待つ 1 歩）のテスト
#
# 実行: bash .claude/tests/scripts/wait-step.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

WAIT="$SCRIPTS_DIR/wait-step.sh"
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"
# 1 歩を 2 秒、打ち切りを 4 秒に縮める（DONE のテストだけ 1 歩を 6 秒にする）
export WAIT_STEP_SECONDS=2 WAIT_STEP_LIMIT=4

it "wait-step: 待つものが終わらなければ、1 歩の時間で WAIT を出して 2 で返る"
log="$TEST_ROOT/job.out"
: >"$log"
out=$("$WAIT" job-a "$log")
assert_eq 2 $?
assert_contains "$out" "WAIT:"

it "wait-step: 待っている background の出力が終わると、すぐに DONE を出して 0 で返る"
( sleep 1; printf 'job output\n[exited with code 0]\n' >>"$log" ) &
start=$(date +%s)
out=$(WAIT_STEP_SECONDS=6 WAIT_STEP_LIMIT=60 "$WAIT" job-b "$log")
assert_eq 0 $?
assert_contains "$out" "DONE: $log"
# 1 歩（6 秒）を待たずに返る
assert_eq 1 "$(( $(date +%s) - start < 5 ))"
wait

it "wait-step: 終わりの印が最後の行でなければ（途中の行に同じ文字列があっても）終わりに数えない"
printf '[exited with code 0]\nstill running\n' >"$TEST_ROOT/mid.out"
out=$("$WAIT" job-mid "$TEST_ROOT/mid.out")
assert_eq 2 $?

it "wait-step: 待ち始めから打ち切りの時間を超えると、待たずに STOP を出して 3 で返る"
: >"$log"
"$WAIT" job-c "$log" >/dev/null
out=$("$WAIT" job-c "$log")
assert_eq 3 $?
assert_contains "$out" "STOP:"

it "wait-step: STOP を出したら待ちを消し、同じ名前の次の待ちは最初から数える"
out=$("$WAIT" job-c "$log")
assert_eq 2 $?

it "wait-step: --end で待ちを消すと、同じ名前の次の待ちは最初から数える"
"$WAIT" job-d "$log" >/dev/null
"$WAIT" job-d "$log" >/dev/null
"$WAIT" --end job-d
out=$("$WAIT" job-d "$log")
assert_eq 2 $?

it "wait-step: 待ちの時計は worktree（git のトップ）ごとに分かれる"
mkdir -p "$TEST_ROOT/wt-a" "$TEST_ROOT/wt-b"
git -C "$TEST_ROOT/wt-a" init -q
git -C "$TEST_ROOT/wt-b" init -q
(cd "$TEST_ROOT/wt-a" && "$WAIT" merge "$log" >/dev/null; "$WAIT" merge "$log" >/dev/null)
out=$(cd "$TEST_ROOT/wt-b" && "$WAIT" merge "$log")
assert_eq 2 $?

it "wait-step: 待ちの名前が無ければ使い方を出して 1 で返る"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage"

finish
