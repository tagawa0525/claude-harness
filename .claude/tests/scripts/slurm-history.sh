#!/usr/bin/env bash
# .claude/scripts/slurm-history.sh（jobcomp の記録から見積もりと実績を比べる）のテスト
#
# 実行: bash .claude/tests/scripts/slurm-history.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

LOG="$TEST_ROOT/job_completions"

# job <id> <name> <state> <limit> <start> <end> <nodes> [<partition>]: jobcomp/filetxt の 1 行（パーティションの既定は main）
job() {
  printf 'JobId=%s UserId=tagawa(1000) GroupId=users(100) Name=%s JobState=%s Partition=%s TimeLimit=%s StartTime=%s EndTime=%s NodeList=r995 NodeCnt=%s ProcCnt=32 WorkDir=/w ReservationName= Tres=cpu=1,mem=86000M,node=1,billing=1 Account= QOS= WcKey= Cluster=unknown SubmitTime=2026-10-11T12:00:00 EligibleTime=2026-10-11T12:00:00 DerivedExitCode=0:0 ExitCode=0:0 \n' \
    "$1" "$2" "$3" "${8:-main}" "$4" "$5" "$6" "$7"
}

{
  job 73 openmc-2A COMPLETED 45 2026-10-11T13:45:32 2026-10-11T14:24:05 1
  job 77 xlc-cancelled CANCELLED 20 2026-10-11T15:09:25 2026-10-11T15:09:25 0
  job 74 openmc-2A COMPLETED 45 2026-10-11T14:24:05 2026-10-11T15:14:32 1
} > "$LOG"

it "ジョブごとに見積もり・実績・比率（実績/見積もり）を出す"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_eq 0 $?
assert_contains "$out" "JOBID"
assert_contains "$out" "RATIO"
line=$(grep -E '^ *73 ' <<<"$out")
assert_contains "$line" "0:45:00"
assert_contains "$line" "0:38:33"
assert_contains "$line" "0.86"

it "実績が見積もりを超えたジョブは比率が 1 を超える"
line=$(grep -E '^ *74 ' <<<"$out")
assert_contains "$line" "0:50:27"
assert_contains "$line" "1.12"

it "NodeCnt=0 のジョブは表に出さず、除いた件数を SKIPPED で出す"
assert_not_contains "$out" "xlc-cancelled"
assert_contains "$out" "SKIPPED: 1"

it "COMPLETED 以外は NodeCnt が 1 以上でも除く"
{
  job 1 ok COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1
  job 2 bad FAILED 10 2026-10-11T10:00:00 2026-10-11T10:01:00 1
  job 3 slow TIMEOUT 10 2026-10-11T10:00:00 2026-10-11T10:10:00 1
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "ok"
assert_not_contains "$out" "bad"
assert_not_contains "$out" "slow"
assert_contains "$out" "SKIPPED: 2"

it "日付をまたぐ実績も求める（月末から翌月）"
job 5 night COMPLETED 120 2026-09-30T23:30:00 2026-10-01T00:30:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "1:00:00"
assert_contains "$out" "0.50"

it "24 時間を超える実績は時を積み上げて出す"
job 6 long COMPLETED 3000 2026-10-10T00:00:00 2026-10-11T02:00:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "26:00:00"

it "見積もりが数値でないジョブ（UNLIMITED）は比率を - にする"
job 7 nolimit COMPLETED UNLIMITED 2026-10-11T10:00:00 2026-10-11T10:05:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_eq 0 $?
line=$(grep -E '^ *7 ' <<<"$out")
assert_contains "$line" "0:05:00"
assert_contains "$line" " -"

it "ジョブ名に空白があっても名前を欠かさず読む"
job 8 "my job" COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "my job"
assert_contains "$out" "0:05:00"

it "--by-name はジョブ名ごとに回数・平均・比率・最大の比率を出す"
{
  job 73 openmc-2A COMPLETED 45 2026-10-11T13:45:32 2026-10-11T14:24:05 1
  job 74 openmc-2A COMPLETED 45 2026-10-11T14:24:05 2026-10-11T15:14:32 1
  job 80 other COMPLETED 10 2026-10-11T16:00:00 2026-10-11T16:05:00 1
  job 81 gone CANCELLED 10 2026-10-11T16:00:00 2026-10-11T16:00:00 0
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" --by-name "$LOG")
assert_eq 0 $?
assert_contains "$out" "RUNS"
assert_contains "$out" "MAX_RATIO"
line=$(grep -E '^ *openmc-2A ' <<<"$out")
assert_contains "$line" " 2 "
assert_contains "$line" "0:45:00"
assert_contains "$line" "0:44:30"
assert_contains "$line" "0.99"
assert_contains "$line" "1.12"
assert_contains "$out" "other"
assert_not_contains "$out" "gone"
assert_contains "$out" "SKIPPED: 1"

it "除く行の時刻が壊れていても（開始前の CANCELLED で StartTime=Unknown）表を出し、SKIPPED に数える"
{
  job 1 ok COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1
  job 2 gone CANCELLED 10 Unknown 2026-10-11T10:05:00 0
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 0 $?
assert_contains "$out" "SKIPPED: 1"

it "終了が開始より前の COMPLETED 行は読めない行として exit 1"
job 3 neg COMPLETED 10 2026-10-11T10:05:00 2026-10-11T10:00:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "1 行目"

it "--by-name の ESTIMATE・ELAPSED・RATIO は見積もりのあるジョブだけで求める（RUNS は全部）"
{
  job 1 a COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1
  job 2 a COMPLETED UNLIMITED 2026-10-11T10:00:00 2026-10-11T12:00:00 1
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" --by-name "$LOG")
line=$(grep -E '^a ' <<<"$out")
assert_contains "$line" " 2 "
assert_contains "$line" "0:10:00"
assert_contains "$line" "0:05:00"
assert_contains "$line" "0.50"

it "--by-name の MAX_RATIO は、最初のジョブの比が 0 でも最大を出す"
{
  job 1 a COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:00:00 1
  job 2 a COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" --by-name "$LOG")
line=$(grep -E '^a ' <<<"$out")
assert_contains "$line" " 0.50"
assert_contains "$line" "0.25"

it "ファイルを 2 つ渡すと exit 1（黙って後ろだけ読まない）"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"

it "存在しない月・時刻（2026-13-01、25 時）の COMPLETED 行は読めない行として exit 1"
job 1 badmonth COMPLETED 10 2026-13-01T10:00:00 2026-13-01T10:05:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "1 行目"
job 1 badhour COMPLETED 10 2026-10-11T25:00:00 2026-10-11T26:00:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "1 行目"

# 割り込まれうるのは、割り込みを r995 に反映した時刻（2026-10-23T21:40）より後に終わったジョブ。ここからのジョブはその後の日付で書く
it "割り込みの後に終わったジョブは、timed のものだけを表に入れ、ほかの件数を UNTIMED で出す"
{
  job 73 bench COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1 timed
  job 74 table COMPLETED 45 2026-11-01T14:24:05 2026-11-01T15:14:32 1 main
  job 75 quick COMPLETED 10 2026-11-01T15:20:00 2026-11-01T15:25:00 1 urgent
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_eq 0 $?
assert_contains "$(grep -E '^ *73 ' <<<"$out")" "0.86"
assert_eq "" "$(grep -E '^ *74 ' <<<"$out")"
assert_eq "" "$(grep -E '^ *75 ' <<<"$out")"
assert_contains "$out" "UNTIMED: 2"

it "割り込みの前に終わったジョブは、main のものも表に入れる"
{
  job 60 before COMPLETED 45 2026-10-11T13:45:32 2026-10-11T14:24:05 1 main
  job 73 bench COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1 timed
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "before"
assert_contains "$out" "bench"
assert_contains "$out" "UNTIMED: 0"

it "割り込みを反映する前に終わったジョブは、同じ日のものも表に入れる"
job 500 sameday COMPLETED 600 2026-10-23T10:00:00 2026-10-23T21:39:59 1 main > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "sameday"
assert_contains "$out" "UNTIMED: 0"

it "割り込みの前に始まり、後に終わった main のジョブは、走っている間に止められうるので表に入れない"
job 501 long COMPLETED 600 2026-10-23T20:00:00 2026-10-23T22:00:00 1 main > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_not_contains "$out" "long"
assert_contains "$out" "UNTIMED: 1"

it "--by-name の集計に、timed でないジョブの時間を混ぜない"
{
  job 73 openmc-2A COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1 timed
  job 74 openmc-2A COMPLETED 45 2026-11-01T14:24:05 2026-11-02T14:24:05 1 main
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" --by-name "$LOG")
line=$(grep -E '^ *openmc-2A ' <<<"$out")
assert_contains "$line" " 1 "
assert_contains "$line" "0:38:33"
assert_contains "$out" "UNTIMED: 1"

it "表に入れなかった COMPLETED 以外の行は、UNTIMED でなく SKIPPED に数える"
{
  job 73 bench COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1 timed
  job 77 gone CANCELLED 20 2026-11-01T15:09:25 2026-11-01T15:09:25 0 main
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "SKIPPED: 1"
assert_contains "$out" "UNTIMED: 0"

it "表に入れるのは、Slurm の設定（modules/slurm/common.nix）の割り込まれないパーティションのジョブ"
part=$(grep -o '"[a-z]* Nodes=[^"]*PreemptMode=OFF' "$CLAUDE_DIR/../modules/slurm/common.nix" | cut -d' ' -f1 | tr -d '"')
assert_ne "" "$part"
job 90 protected COMPLETED 10 2026-11-01T10:00:00 2026-11-01T10:05:00 1 "$part" > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "protected"
assert_contains "$out" "UNTIMED: 0"

it "割り込みの後に終わった COMPLETED の行にパーティションが無ければ、どちらに入れるか決められないので行番号つきで exit 1"
echo "JobId=80 Name=nopart JobState=COMPLETED TimeLimit=10 StartTime=2026-11-01T10:00:00 EndTime=2026-11-01T10:05:00 NodeCnt=1" > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "1 行目"
assert_contains "$out" "Partition"

it "ファイルが無ければ理由を出して exit 1"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$TEST_ROOT/none" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"

it "読めない行（開始時刻がない）があれば行番号つきで exit 1"
{
  job 1 ok COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1
  echo "JobId=2 Name=broken JobState=COMPLETED TimeLimit=10 NodeCnt=1"
} > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "2 行目"

it "不明なオプションは exit 1"
out=$("$SCRIPTS_DIR/slurm-history.sh" --nope "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"

finish
