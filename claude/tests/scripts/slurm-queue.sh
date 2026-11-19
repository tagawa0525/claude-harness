#!/usr/bin/env bash
# claude/scripts/slurm-queue.sh（Slurm の待ち行列を区分と目的つきで並べる）の配備の確認
#
# 実行: bash claude/tests/scripts/slurm-queue.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script slurm-queue（ADR-0101）。並べ方の場合分けは crates/claude-hooks/src/script/slurm_queue.rs の
# 単体のテストが、squeue の読み方と終了コードは crates/claude-hooks/tests/script_slurm_queue.rs が確かめ、ここでは sh から
# ブランチでビルドしたバイナリを呼べることと、一覧、--order、squeue の失敗の終了コードを確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

# squeue の偽物。FAKE_SQUEUE_OUT を出して FAKE_SQUEUE_EXIT で終わる
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/squeue" <<'EOF'
#!/usr/bin/env bash
[[ -z ${FAKE_SQUEUE_OUT:-} ]] || printf '%s\n' "$FAKE_SQUEUE_OUT"
exit "${FAKE_SQUEUE_EXIT:-0}"
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH"

QUEUE="$SCRIPTS_DIR/slurm-queue.sh"
C="feat/x /home/u/github/xlc-feat-x claude=abc"
LINES="$(printf '%s\n' \
  "20|main|trial-job|PENDING|10010|10:00|N/A|trial | #1 | p | $C" \
  "21|main|record-job|PENDING|10005|10:00|N/A|record | #2 | p | $C")"

it "slurm-queue: sh から、ジョブの行と、区分の順に並べる ORDER: の行を出す"
out=$(FAKE_SQUEUE_OUT="$LINES" "$QUEUE")
assert_eq 0 $?
assert_contains "$out" "20 PENDING main 10010 trial 10:00 N/A trial-job #1"
assert_contains "$out" "ORDER: main: 21（record）が 20（trial）の後ろで待っている。scontrol top 21,20"

it "slurm-queue: sh から、--order で並べ直す番号の列を出す"
out=$(FAKE_SQUEUE_OUT="$LINES" "$QUEUE" --order)
assert_eq 0 $?
assert_eq "21,20" "$out"

it "slurm-queue: squeue が失敗したら、その終了コードで終わる"
out=$(FAKE_SQUEUE_EXIT=3 "$QUEUE" 2>&1)
assert_eq 3 $?
assert_contains "$out" "ERROR:"

finish
