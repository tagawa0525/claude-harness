#!/usr/bin/env bash
# claude/scripts/slurm-history.sh が sacct の記録も読み、MaxRSS を並べ、排他でないジョブを外すことのテスト
#
# 実行: bash claude/tests/scripts/slurm-history-sacct.sh（まとめて実行: bash claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

# sacct の偽物。引数を記録し、-j の無い呼び出しには $FAKE_SACCT_ALL を出す。FAKE_SACCT_FAIL があれば失敗する
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/sacct" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TEST_ROOT/sacct.args"
[[ -z ${FAKE_SACCT_FAIL:-} ]] || exit 1
cat "$FAKE_SACCT_ALL"
FAKE
chmod +x "$FAKE_BIN/sacct"
export PATH="$FAKE_BIN:$PATH" TEST_ROOT FAKE_SACCT_ALL="$TEST_ROOT/sacct.all"

HISTORY="$SCRIPTS_DIR/slurm-history.sh"
LOG="$TEST_ROOT/job_completions"
US=$'\x1f'

# comp <id> <name> <start> <end>: jobcomp の 1 行（COMPLETED、見積もり 60 分、timed）
comp() {
  printf 'JobId=%s Name=%s JobState=COMPLETED Partition=timed TimeLimit=60 StartTime=%s EndTime=%s NodeCnt=1 ProcCnt=32 \n' "$1" "$2" "$3" "$4"
}

# row <JobID> <名前> <状態> <Start> <End> <MaxRSS> <SubmitLine>: sacct の 1 行
# 欄: JobID,JobName,State,Partition,TimelimitRaw,Start,End,MaxRSS,SubmitLine（区切りは \x1f。見積もり 60 分、timed）
row() {
  printf '%s\n' "$1${US}$2${US}$3${US}timed${US}60${US}$4${US}$5${US}$6${US}$7"
}

EXCL='/nix/store/x/bin/sbatch --exclusive --partition=timed --wrap run'
SHARED='/nix/store/x/bin/sbatch --cpus-per-task=8 --partition=timed --wrap run'

it "sacct のジョブを、.batch の MaxRSS（ステップの最大）つきで出す"
{
  row 201 lattice COMPLETED 2026-11-01T10:00:00 2026-11-01T10:30:00 "" "$EXCL"
  row 201.batch batch COMPLETED 2026-11-01T10:00:00 2026-11-01T10:30:00 512M ""
  row 201.0 step COMPLETED 2026-11-01T10:00:00 2026-11-01T10:30:00 2048M ""
} >"$FAKE_SACCT_ALL"
: >"$LOG"
out=$("$HISTORY" --sacct "$LOG")
assert_eq 0 $?
line=$(grep -E '^ *201 ' <<<"$out")
assert_contains "$line" "0:30:00"
assert_contains "$line" "0.50"
assert_contains "$line" "2.0G"
assert_contains "$out" "MAX_RSS"

it "sacct の記録が呼ぶ引数に、開始日（-S）とステップを含める（-X を付けない）"
args=$(<"$TEST_ROOT/sacct.args")
assert_contains "$args" "-S"
assert_not_contains "$args" "-X"

it "--by-name は MaxRSS の最大を出し、sacct より前の jobcomp の分も重ねる"
{
  comp 100 lattice 2026-10-05T10:00:00 2026-10-05T10:10:00
  comp 201 lattice 2026-11-01T10:00:00 2026-11-01T10:30:00
} >"$LOG"
out=$("$HISTORY" --sacct --by-name "$LOG")
line=$(grep -E '^lattice ' <<<"$out")
assert_contains "$line" " 2 "
assert_contains "$line" "2.0G"

it "sacct と jobcomp の両方にあるジョブは 1 回だけ数える"
assert_not_contains "$line" " 3 "

it "排他でないジョブ（--cpus-per-task）は表に入れず、SHARED に数える"
{
  row 202 lattice-shared COMPLETED 2026-11-01T11:00:00 2026-11-01T11:27:00 "" "$SHARED"
  row 202.batch batch COMPLETED 2026-11-01T11:00:00 2026-11-01T11:27:00 100M ""
} >"$FAKE_SACCT_ALL"
: >"$LOG"
out=$("$HISTORY" --sacct --by-name "$LOG")
assert_not_contains "$out" "lattice-shared"
assert_contains "$out" "SHARED: 1"

it "排他か分からないジョブ（slurm-run.sh を通らない）は表に入れる"
row 203 manual COMPLETED 2026-11-01T11:00:00 2026-11-01T11:27:00 "" '/usr/bin/sbatch --wrap run' >"$FAKE_SACCT_ALL"
out=$("$HISTORY" --sacct "$LOG")
assert_contains "$out" "manual"

it "SubmitLine が改行を含んでも、続きの行を別のジョブと読まない"
{
  row 204 multi COMPLETED 2026-11-01T11:00:00 2026-11-01T11:27:00 "" "$EXCL"$'\n'"  more lines"
} >"$FAKE_SACCT_ALL"
out=$("$HISTORY" --sacct "$LOG" 2>&1)
assert_eq 0 $?
assert_contains "$out" "multi"

it "MaxRSS が無い（jobcomp のジョブ、.batch の行が無いジョブ）は -"
{
  comp 100 old 2026-10-05T10:00:00 2026-10-05T10:10:00
} >"$LOG"
out=$("$HISTORY" --sacct "$LOG")
line=$(grep -E '^ *100 ' <<<"$out")
assert_eq "-" "${line##* }"

it "sacct が失敗したら ERROR を出して exit 1"
out=$(FAKE_SACCT_FAIL=1 "$HISTORY" --sacct "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"

it "FILE だけを渡したときは sacct を呼ばない（ADR-0019 の定義のまま）"
rm -f "$TEST_ROOT/sacct.args"
"$HISTORY" "$LOG" >/dev/null
assert_file_missing "$TEST_ROOT/sacct.args"

finish
