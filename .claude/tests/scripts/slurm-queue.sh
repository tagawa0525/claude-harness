#!/usr/bin/env bash
# .claude/scripts/slurm-queue.sh（Slurm の待ち行列を区分と目的つきで並べる）のテスト
#
# 実行: bash .claude/tests/scripts/slurm-queue.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

# squeue の偽物。引数を記録し、FAKE_SQUEUE_OUT を出して FAKE_SQUEUE_EXIT で終わる
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/squeue" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TEST_ROOT/squeue.args"
[[ -z ${FAKE_SQUEUE_OUT:-} ]] || printf '%s\n' "$FAKE_SQUEUE_OUT"
exit "${FAKE_SQUEUE_EXIT:-0}"
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH" TEST_ROOT

QUEUE="$SCRIPTS_DIR/slurm-queue.sh"
C="feat/x /home/u/github/xlc-feat-x claude=abc"

it "slurm-queue: squeue を見出しなしで、区切りの | と、番号、パーティション、名前、状態、優先度、見積もり、開始の見込み、コメントの順で読む"
"$QUEUE" >/dev/null
args=$(cat "$TEST_ROOT/squeue.args")
assert_contains "$args" "--noheader"
assert_contains "$args" $'--format\n%i|%P|%j|%T|%Q|%l|%S|%k'

it "slurm-queue: ジョブごとに番号、状態、パーティション、優先度、区分、見積もり、開始の見込み、名前、参照と、目的、連絡先を出す"
out=$(FAKE_SQUEUE_OUT="601|main|xlc-small-core|PENDING|10030|2:30:00|2026-10-30T03:00:00|decision | #0200 | 平衡キセノンで振動が消えるかで、修正方法を決める | $C" "$QUEUE")
assert_eq 0 $?
assert_contains "$out" "601 PENDING main 10030 decision 2:30:00 2026-10-30T03:00:00 xlc-small-core #0200"
assert_contains "$out" "  目的: 平衡キセノンで振動が消えるかで、修正方法を決める"
assert_contains "$out" "  連絡先: $C"

it "slurm-queue: 連絡先に | があっても、目的までの 3 つの区切りのあとをすべて連絡先にする"
out=$(FAKE_SQUEUE_OUT="601|main|x|PENDING|10030|30:00|N/A|trial | - 環境の確認 | 確認だけ | feat/a|b /p" "$QUEUE")
assert_contains "$out" "601 PENDING main 10030 trial 30:00 N/A x - 環境の確認"
assert_contains "$out" "  連絡先: feat/a|b /p"

it "slurm-queue: 区分と目的の無いジョブ（slurm-run.sh の外から投げたものなど）は、区分を - にして、コメントを連絡先に出す"
out=$(FAKE_SQUEUE_OUT="590|main|old-job|RUNNING|10000|20:00|2026-10-29T08:43:25|$C" "$QUEUE")
assert_contains "$out" "590 RUNNING main 10000 - 20:00 2026-10-29T08:43:25 old-job -"
assert_contains "$out" "  目的: （無い）"
assert_contains "$out" "  連絡先: $C"

it "slurm-queue: timed のパーティションを先に、その中は走っているもの、止められたもの、待っているもの（優先度の高い順、同じなら番号の順）の順に並べる"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "12|main|m-pend-low|PENDING|10001|10:00|N/A|trial | #1 | p | $C" \
  "11|main|m-pend-high|PENDING|10005|10:00|N/A|trial | #1 | p | $C" \
  "13|main|m-pend-tie|PENDING|10005|10:00|N/A|trial | #1 | p | $C" \
  "10|main|m-susp|SUSPENDED|10000|10:00|N/A|trial | #1 | p | $C" \
  "14|timed|t-pend|PENDING|10000|10:00|N/A|timed | #1 | p | $C" \
  "9|main|m-run|RUNNING|10000|10:00|N/A|trial | #1 | p | $C" \
  "15|timed|t-run|RUNNING|10000|10:00|N/A|timed | #1 | p | $C")" "$QUEUE")
order=$(awk '/^[0-9]/ { printf "%s ", $1 }' <<<"$out")
assert_eq "15 14 9 10 11 13 12 " "$order"

it "slurm-queue: 待っている上の区分のジョブが、同じパーティションの下の区分のジョブの後ろにあれば、ORDER: の行で区分の順に並べるコマンドを示す"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "20|main|trial-job|PENDING|10010|10:00|N/A|trial | #1 | p | $C" \
  "21|main|record-job|PENDING|10005|10:00|N/A|record | #2 | p | $C")" "$QUEUE")
assert_contains "$out" "ORDER: main: 21（record）が 20（trial）の後ろで待っている。scontrol top 21,20"

it "slurm-queue: 区分の順は timed、record、decision、trial"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "30|main|a|PENDING|10040|10:00|N/A|trial | #1 | p | $C" \
  "31|main|b|PENDING|10030|10:00|N/A|decision | #1 | p | $C" \
  "32|main|c|PENDING|10020|10:00|N/A|record | #1 | p | $C" \
  "33|main|d|PENDING|10010|10:00|N/A|timed | #1 | p | $C")" "$QUEUE")
assert_contains "$out" "31（decision）が 30（trial）の後ろ"
assert_contains "$out" "32（record）が 31（decision）の後ろ"
assert_contains "$out" "33（timed）が 32（record）の後ろ"
assert_contains "$out" "scontrol top 33,32,31,30"
assert_eq 1 "$(grep -c '^ORDER:' <<<"$out")"

it "slurm-queue: 並べるコマンドは、上の区分のジョブより前に出してはならないジョブを追い越させない（scontrol top は並べた順に前に出す）"
# record が先頭で、その後ろの decision が trial の後ろにある。decision だけを前に出すと record を追い越す
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "70|main|r1|PENDING|10030|10:00|N/A|record | #1 | p | $C" \
  "71|main|t|PENDING|10020|10:00|N/A|trial | #1 | p | $C" \
  "72|main|d|PENDING|10010|10:00|N/A|decision | #1 | p | $C")" "$QUEUE")
assert_contains "$out" "ORDER: main: 72（decision）が 71（trial）の後ろで待っている。scontrol top 70,72,71"

it "slurm-queue: 同じ優先度なら番号の順で後ろにあるかを決める"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "41|main|record-job|PENDING|10005|10:00|N/A|record | #2 | p | $C" \
  "40|main|trial-job|PENDING|10005|10:00|N/A|trial | #1 | p | $C")" "$QUEUE")
assert_contains "$out" "ORDER: main: 41（record）が 40（trial）の後ろで待っている。scontrol top 41,40"

it "slurm-queue: 区分の順に並んでいれば、ORDER: の行を出さない"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "50|main|record-job|PENDING|10010|10:00|N/A|record | #2 | p | $C" \
  "51|main|trial-job|PENDING|10005|10:00|N/A|trial | #1 | p | $C")" "$QUEUE")
assert_not_contains "$out" "ORDER:"

it "slurm-queue: 走っているジョブ、別のパーティションのジョブ、区分の無いジョブとは順を比べない"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "60|main|trial-run|RUNNING|10000|10:00|N/A|trial | #1 | p | $C" \
  "61|main|record-pend|PENDING|10000|10:00|N/A|record | #2 | p | $C" \
  "62|timed|trial-timed|PENDING|10050|10:00|N/A|trial | #1 | p | $C" \
  "63|main|old|PENDING|10050|10:00|N/A|$C")" "$QUEUE")
assert_not_contains "$out" "ORDER:"

it "slurm-queue: 並べるコマンドで後ろに回る、区分の無い待っているジョブを ORDER: の行に挙げる"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "90|main|old|PENDING|10060|10:00|N/A|$C" \
  "91|main|t|PENDING|10050|10:00|N/A|trial | #1 | p | $C" \
  "92|main|r|PENDING|10040|10:00|N/A|record | #1 | p | $C")" "$QUEUE")
assert_contains "$out" "ORDER: main: 92（record）が 91（trial）の後ろで待っている。scontrol top 92,91（区分の無い 90 は後ろに回る）"

it "slurm-queue: 走り始めたジョブ（CONFIGURING、COMPLETING）は、走っているジョブと同じく待っているジョブより前に並べる"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "80|main|pend|PENDING|10050|10:00|N/A|trial | #1 | p | $C" \
  "81|main|conf|CONFIGURING|10000|10:00|N/A|trial | #1 | p | $C")" "$QUEUE")
order=$(awk '/^[0-9]/ { printf "%s ", $1 }' <<<"$out")
assert_eq "81 80 " "$order"

it "slurm-queue: 待ち行列が空なら、空と出して 0 で終わる"
out=$("$QUEUE")
assert_eq 0 $?
assert_contains "$out" "待ち行列にジョブが無い"

it "slurm-queue: squeue が失敗したら、その終了コードで終わる"
out=$(FAKE_SQUEUE_EXIT=3 "$QUEUE" 2>&1)
assert_eq 3 $?
assert_contains "$out" "ERROR:"

it "slurm-queue: 引数を受け付けない"
out=$("$QUEUE" --all 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

finish
