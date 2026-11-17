#!/usr/bin/env bash
# claude/scripts/wait-step.sh（/delegate の担当が、ターンを終えずに待つ 1 歩）の配備の確認
#
# 実行: bash claude/tests/scripts/wait-step.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script wait-step。判定の場合分けは crates/claude-hooks/tests/script_wait_step.rs が確かめ、ここでは
# sh からブランチでビルドしたバイナリを呼べることと、出力の行の形を確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

WAIT="$SCRIPTS_DIR/wait-step.sh"
export TMPDIR="$TEST_ROOT/tmp"
mkdir -p "$TMPDIR"
export WAIT_STEP_SECONDS=1 WAIT_STEP_REPORT=60
log="$TEST_ROOT/job.out"
: >"$log"

it "wait-step: 待つものが終わらなければ、1 歩の時間で WAIT と次の行動を出して 2 で返る"
out=$("$WAIT" /wt/a "$log")
assert_eq 2 $?
assert_contains "$out" "WAIT:"
assert_contains "$out" "NEXT: ターンを終えずに、同じ引数でもう一度呼ぶ"

it "wait-step: 待っている background の出力が終わると、DONE と残りの呼び直しと --end のコマンドを出して 0 で返る"
printf 'job output\n[exited with code 0]\n' >"$TEST_ROOT/done.out"
out=$("$WAIT" /wt/b "$log" "$TEST_ROOT/done.out")
assert_eq 0 $?
assert_contains "$out" "DONE: $TEST_ROOT/done.out"
assert_contains "$out" "~/.claude/scripts/wait-step.sh /wt/b $log で呼び直し"
assert_contains "$out" "~/.claude/scripts/wait-step.sh --end /wt/b"

it "wait-step: 途中の報告から間隔を超えると、REPORT と親に送る文面を出して 3 で返る"
WAIT_STEP_REPORT=2 "$WAIT" /wt/c "$log" >/dev/null
out=$(WAIT_STEP_SECONDS=2 WAIT_STEP_REPORT=2 "$WAIT" /wt/c "$log")
assert_eq 3 $?
assert_contains "$out" "REPORT:"
assert_contains "$out" "<待つもの> の待ちが 55 分を超えた。終わり次第、残りの作業を続ける"

it "wait-step: --end で待ちを消して 0 で返る"
"$WAIT" --end /wt/c
assert_eq 0 $?

it "wait-step: 鍵が無ければ使い方を出して 1 で返る"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage"

finish
