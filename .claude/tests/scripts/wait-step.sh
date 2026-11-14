#!/usr/bin/env bash
# .claude/scripts/wait-step.sh（/delegate の担当が、ターンを終えずに待つ 1 歩）のテスト
#
# 実行: bash .claude/tests/scripts/wait-step.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

WAIT="$SCRIPTS_DIR/wait-step.sh"
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"
# 1 歩を 2 秒、途中の報告の間隔を 3 秒に縮める
export WAIT_STEP_SECONDS=2 WAIT_STEP_REPORT=3
log="$TEST_ROOT/job.out"
: >"$log"

it "wait-step: 待つものが終わらなければ、1 歩の時間で WAIT を出して 2 で返る"
out=$("$WAIT" b-job-a "$log")
assert_eq 2 $?
assert_contains "$out" "WAIT:"

it "wait-step: 待っている background の出力が終わると、すぐに DONE を出して 0 で返る"
done_log="$TEST_ROOT/done.out"
: >"$done_log"
( sleep 1; printf 'job output\n[exited with code 0]\n' >>"$done_log" ) &
start=$(date +%s)
out=$(WAIT_STEP_SECONDS=6 WAIT_STEP_REPORT=60 "$WAIT" b-job-b "$log" "$done_log")
assert_eq 0 $?
assert_contains "$out" "DONE: $done_log"
# 1 歩（6 秒）を待たずに返る
assert_eq 1 "$(( $(date +%s) - start < 5 ))"
wait

it "wait-step: 終わりの印が最後の行でなければ（途中の行に同じ文字列があっても）終わりに数えない"
printf '[exited with code 0]\nstill running\n' >"$TEST_ROOT/mid.out"
out=$("$WAIT" b-job-mid "$TEST_ROOT/mid.out")
assert_eq 2 $?

it "wait-step: 時計が報告の間隔を超えると REPORT を出して 3 で返り、時計を数え直して待ちを続けられる"
"$WAIT" b-job-c "$log" >/dev/null
out=$("$WAIT" b-job-c "$log")
assert_eq 3 $?
assert_contains "$out" "REPORT:"
# 数え直したので、次の 1 歩は報告の間隔に届かず WAIT
out=$("$WAIT" b-job-c "$log")
assert_eq 2 $?

it "wait-step: --end で待ちを消すと、同じ名前の次の待ちは最初から数える"
"$WAIT" b-job-d "$log" >/dev/null
"$WAIT" --end b-job-d
out=$("$WAIT" b-job-d "$log")
assert_eq 2 $?
# --end しなければ、同じ 2 回で報告の間隔を超える
"$WAIT" b-job-e "$log" >/dev/null
out=$("$WAIT" b-job-e "$log")
assert_eq 3 $?

it "wait-step: 待ちの時計は名前ごとに分かれる（日本語の名前も重ならない）"
"$WAIT" "実験-merge" "$log" >/dev/null
out=$("$WAIT" "計算-merge" "$log")
assert_eq 2 $?

it "wait-step: 前の呼び出しから 1 歩の 2 倍より長く空いたら、消し忘れた待ちとして数え直す"
WAIT_STEP_SECONDS=1 "$WAIT" b-job-f "$log" >/dev/null
sleep 3
out=$(WAIT_STEP_SECONDS=1 "$WAIT" b-job-f "$log")
assert_eq 2 $?

it "wait-step: 渡した出力のファイルが無ければ、理由を出して 1 で返る"
out=$("$WAIT" b-job-g "$TEST_ROOT/no-such.out" 2>&1)
assert_eq 1 $?
assert_contains "$out" "no-such.out"

it "wait-step: 待ちの名前が無ければ使い方を出して 1 で返る"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage"

finish
