#!/usr/bin/env bash
# .claude/scripts/wait-step.sh（/delegate の担当が、ターンを終えずに待つ 1 歩）のテスト
#
# 実行: bash .claude/tests/scripts/wait-step.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

WAIT="$SCRIPTS_DIR/wait-step.sh"
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"
# 1 歩を 2 秒、途中の報告の間隔を 3 秒、--limit の 1 分を 1 秒に縮める
export WAIT_STEP_SECONDS=2 WAIT_STEP_REPORT=3 WAIT_STEP_MINUTE=1
log="$TEST_ROOT/job.out"
: >"$log"

it "wait-step: 待つものが終わらなければ、1 歩の時間で WAIT を出して 2 で返る"
out=$(WAIT_STEP_REPORT=60 "$WAIT" b-a "$log")
assert_eq 2 $?
assert_contains "$out" "WAIT:"

it "wait-step: 待っている background の出力が終わると（kill されても）、すぐに DONE を出して 0 で返る"
for end in '[exited with code 0]' '[killed]'; do
  done_log="$TEST_ROOT/done.out"
  : >"$done_log"
  ( sleep 1; printf 'job output\n%s\n' "$end" >>"$done_log" ) &
  start=$(date +%s)
  out=$(WAIT_STEP_SECONDS=6 WAIT_STEP_REPORT=60 "$WAIT" "b-b$end" "$log" "$done_log")
  assert_eq 0 $?
  assert_contains "$out" "DONE: $done_log"
  # 1 歩（6 秒）を待たずに返る
  assert_eq 1 "$(( $(date +%s) - start < 5 ))"
  wait
done

it "wait-step: 終わりの印が最後の行でなければ（途中の行に同じ文字列があっても）終わりに数えない"
printf '[exited with code 0]\nstill running\n' >"$TEST_ROOT/mid.out"
out=$(WAIT_STEP_REPORT=60 "$WAIT" b-mid "$TEST_ROOT/mid.out")
assert_eq 2 $?

it "wait-step: 途中の報告から間隔を超えると REPORT を出して 3 で返り、数え直して待ちを続けられる"
"$WAIT" b-c "$log" >/dev/null
out=$("$WAIT" b-c "$log")
assert_eq 3 $?
assert_contains "$out" "REPORT:"
out=$("$WAIT" b-c "$log")
assert_eq 2 $?

it "wait-step: 待つものがもう終わっていれば、途中の報告の時刻を過ぎていても DONE を先に出す"
"$WAIT" b-d "$log" >/dev/null
printf '[exited with code 0]\n' >"$TEST_ROOT/ended.out"
sleep 2
out=$("$WAIT" b-d "$log" "$TEST_ROOT/ended.out")
assert_eq 0 $?

it "wait-step: --limit の分を待ち始めから超えると STOP を出して 4 で返る（途中の報告で数え直さない）"
WAIT_STEP_REPORT=60 "$WAIT" b-e --limit 3 >/dev/null
out=$(WAIT_STEP_REPORT=60 "$WAIT" b-e --limit 3)
assert_eq 4 $?
assert_contains "$out" "STOP:"

it "wait-step: --end で待ちを消すと、同じ名前の次の待ちは最初から数える"
"$WAIT" b-f "$log" >/dev/null
"$WAIT" --end b-f
out=$("$WAIT" b-f "$log")
assert_eq 2 $?

it "wait-step: 待ちの時計は名前ごとに分かれる（日本語の名前も重ならない）"
"$WAIT" "実験" "$log" >/dev/null
out=$("$WAIT" "計算" "$log")
assert_eq 2 $?

it "wait-step: 渡した出力のファイルが無ければ、理由を出して 1 で返る"
out=$("$WAIT" b-g "$TEST_ROOT/no-such.out" 2>&1)
assert_eq 1 $?
assert_contains "$out" "no-such.out"

it "wait-step: 待ちの状態を書けなければ、理由を出して 1 で返る"
out=$(TMPDIR=/proc/no-such "$WAIT" b-h "$log" 2>&1)
assert_eq 1 $?
assert_contains "$out" "状態を書けません"

it "wait-step: 待ちの名前が無いか --limit が数でなければ、使い方を出して 1 で返る"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage"
out=$("$WAIT" b-i --limit x 2>&1)
assert_eq 1 $?

finish
