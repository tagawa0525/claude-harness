#!/usr/bin/env bash
# claude/scripts/wait-step.sh（/delegate の担当が、ターンを終えずに待つ 1 歩）のテスト
#
# 実行: bash claude/tests/scripts/wait-step.sh（まとめて実行: bash claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

WAIT="$SCRIPTS_DIR/wait-step.sh"
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"
# 1 歩を 2 秒、途中の報告の間隔を 3 秒に縮める
export WAIT_STEP_SECONDS=2 WAIT_STEP_REPORT=3
log="$TEST_ROOT/job.out"
: >"$log"

it "wait-step: 待つものが終わらなければ、1 歩の時間で WAIT を出して 2 で返る"
out=$(WAIT_STEP_REPORT=60 "$WAIT" /wt/a "$log")
assert_eq 2 $?
assert_contains "$out" "WAIT:"

it "wait-step: 待っている background の出力が終わると（kill されても）、すぐに DONE を出して 0 で返る"
for end in '[exited with code 0]' '[killed]'; do
  done_log="$TEST_ROOT/done.out"
  : >"$done_log"
  ( sleep 1; printf 'job output\n%s\n' "$end" >>"$done_log" ) &
  start=$(date +%s)
  out=$(WAIT_STEP_SECONDS=6 WAIT_STEP_REPORT=60 "$WAIT" "/wt/b$end" "$log" "$done_log")
  assert_eq 0 $?
  assert_contains "$out" "DONE: $done_log"
  # 1 歩（6 秒）を待たずに返る
  assert_eq 1 "$(( $(date +%s) - start < 5 ))"
  wait
done

it "wait-step: 終わりの印が最後の行でなければ（途中の行に同じ文字列があっても）終わりに数えない"
printf '[exited with code 0]\nstill running\n' >"$TEST_ROOT/mid.out"
out=$(WAIT_STEP_REPORT=60 "$WAIT" /wt/mid "$TEST_ROOT/mid.out")
assert_eq 2 $?

it "wait-step: 途中の報告から間隔を超えると REPORT を出して 3 で返り、数え直して待ちを続けられる"
"$WAIT" /wt/c "$log" >/dev/null
out=$("$WAIT" /wt/c "$log")
assert_eq 3 $?
assert_contains "$out" "REPORT:"
out=$("$WAIT" /wt/c "$log")
assert_eq 2 $?

it "wait-step: DONE の後に間を置いて呼び直しても、時計は途中の報告から続く"
WAIT_STEP_REPORT=5 "$WAIT" /wt/d "$log" >/dev/null
printf '[exited with code 0]\n' >"$TEST_ROOT/ended.out"
WAIT_STEP_REPORT=5 "$WAIT" /wt/d "$log" "$TEST_ROOT/ended.out" >/dev/null
sleep 2
# 待ち始めから 2 + 2 = 4 秒以上、報告の間隔 5 秒の手前。次の 1 歩で報告の時刻に届く
out=$(WAIT_STEP_REPORT=5 "$WAIT" /wt/d "$log")
assert_eq 3 $?

it "wait-step: 待つものがもう終わっていれば、途中の報告の時刻を過ぎていても DONE を先に出す"
"$WAIT" /wt/e "$log" >/dev/null
sleep 2
out=$("$WAIT" /wt/e "$log" "$TEST_ROOT/ended.out")
assert_eq 0 $?

it "wait-step: 前の呼び出しから報告の間隔を超えて空いていれば、終わった待ちとして数え直す"
WAIT_STEP_SECONDS=1 "$WAIT" /wt/f "$log" >/dev/null
sleep 4
out=$(WAIT_STEP_SECONDS=1 "$WAIT" /wt/f "$log")
assert_eq 2 $?

it "wait-step: --end で待ちを消すと、同じ鍵の次の待ちは最初から数える"
"$WAIT" /wt/g "$log" >/dev/null
"$WAIT" --end /wt/g
out=$("$WAIT" /wt/g "$log")
assert_eq 2 $?

it "wait-step: 時計は鍵（worktree のパス）ごとに分かれる（日本語のパスも重ならない）"
"$WAIT" "/実験/x" "$log" >/dev/null
out=$("$WAIT" "/計算/x" "$log")
assert_eq 2 $?

it "wait-step: 鍵のパスは正規化する（末尾の / があっても同じ時計）"
"$WAIT" /wt/n "$log" >/dev/null
out=$("$WAIT" /wt/n/ "$log")
assert_eq 3 $?

it "wait-step: WAIT の次の行で、ターンを終えずに同じ引数で呼び直すことと、届いた通知で続けることを示す"
out=$(WAIT_STEP_REPORT=60 "$WAIT" /wt/next-wait "$log")
assert_eq 2 $?
assert_contains "$out" "NEXT: ターンを終えずに、同じ引数でもう一度呼ぶ"
assert_contains "$out" "通知（終わりの通知、親の返事）"

it "wait-step: DONE の次の行で、残りの出力のファイルで呼び直すコマンドと、すべて済んだときの --end のコマンドを示す"
printf '[exited with code 0]\n' >"$TEST_ROOT/fin.out"
out=$(WAIT_STEP_REPORT=60 "$WAIT" "/wt/next done" "$log" "$TEST_ROOT/fin.out")
assert_eq 0 $?
assert_contains "$out" "DONE: $TEST_ROOT/fin.out"
assert_contains "$out" "~/.claude/scripts/wait-step.sh /wt/next\\ done $log"
assert_not_contains "$out" "wait-step.sh /wt/next\\ done $log $TEST_ROOT/fin.out"
assert_contains "$out" "~/.claude/scripts/wait-step.sh --end /wt/next\\ done"

it "wait-step: 最後の出力のファイルが終われば、DONE の次の行は出力のファイルの無い呼び直しを示す"
out=$(WAIT_STEP_REPORT=60 "$WAIT" /wt/next-last "$TEST_ROOT/fin.out")
assert_eq 0 $?
assert_contains "$out" "出力のファイルの無い待ち"
assert_contains "$out" "~/.claude/scripts/wait-step.sh /wt/next-last"
assert_contains "$out" "~/.claude/scripts/wait-step.sh --end /wt/next-last"

it "wait-step: REPORT の次の行で、親への途中の報告の宛先と文面を示し、待ちを続けさせる"
"$WAIT" /wt/next-report "$log" >/dev/null
out=$("$WAIT" /wt/next-report "$log")
assert_eq 3 $?
assert_contains "$out" 'SendMessage（to: "main"）'
assert_contains "$out" "<待つもの> の待ちが 55 分を超えた。終わり次第、残りの作業を続ける"
assert_contains "$out" "同じ引数でもう一度呼ぶ"
assert_contains "$out" "止めるかは親の返事で決める"

it "wait-step: 渡した出力のファイルが無ければ、理由を出して 1 で返る"
out=$("$WAIT" /wt/h "$TEST_ROOT/no-such.out" 2>&1)
assert_eq 1 $?
assert_contains "$out" "no-such.out"

it "wait-step: 待ちの状態を書けなければ、理由を出して 1 で返る"
out=$(TMPDIR=/proc/no-such "$WAIT" /wt/i "$log" 2>&1)
assert_eq 1 $?
assert_contains "$out" "状態を書けません"

it "wait-step: 鍵が無ければ使い方を出して 1 で返る"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage"

finish
