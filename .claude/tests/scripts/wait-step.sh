#!/usr/bin/env bash
# .claude/scripts/wait-step.sh（/delegate の担当が、ターンを終えずに待つ 1 歩）のテスト
#
# 実行: bash .claude/tests/scripts/wait-step.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

WAIT="$SCRIPTS_DIR/wait-step.sh"
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"
# 1 歩を 2 秒、打ち切りを 4 秒に縮める
export WAIT_STEP_SECONDS=2 WAIT_STEP_LIMIT=4

it "wait-step: 待つものが終わらなければ、1 歩の時間で WAIT を出して 2 で返る"
log="$TEST_ROOT/job.out"
: >"$log"
out=$("$WAIT" job-a "$log")
assert_eq 2 $?
assert_contains "$out" "WAIT:"

it "wait-step: 待っている background の出力が終わると、すぐに DONE を出して 0 で返る"
( sleep 1; echo "[exited with code 0]" >>"$log" ) &
start=$(date +%s)
out=$("$WAIT" job-b "$log")
assert_eq 0 $?
assert_contains "$out" "DONE: $log"
# 1 歩（2 秒）を待たずに返る
assert_eq 1 "$(( $(date +%s) - start < 2 ))"
wait

it "wait-step: 待ち始めから打ち切りの時間を超えると、待たずに STOP を出して 3 で返る"
: >"$log"
"$WAIT" job-c "$log" >/dev/null
"$WAIT" job-c "$log" >/dev/null
out=$("$WAIT" job-c "$log")
assert_eq 3 $?
assert_contains "$out" "STOP:"

it "wait-step: --end で待ちを消すと、同じ名前の次の待ちは最初から数える"
"$WAIT" --end job-c
out=$("$WAIT" job-c "$log")
assert_eq 2 $?

it "wait-step: 待ちの名前が無ければ使い方を出して 1 で返る"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage"

finish
