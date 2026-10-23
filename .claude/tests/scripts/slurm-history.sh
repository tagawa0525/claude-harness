#!/usr/bin/env bash
# .claude/scripts/slurm-history.sh（jobcomp の記録から見積もりと実績を比べる）のテスト
#
# 実行: bash .claude/tests/scripts/slurm-history.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

LOG="$TEST_ROOT/job_completions"

# sacct の偽物。引数を記録し、FAKE_SACCT_ROWS（`<番号>|<開始>|<止められていた時間>` の改行区切り）をそのまま出す。
# 無ければ何も出さない（どのジョブも accounting に無い）。FAKE_SACCT_EXIT で失敗させる
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/sacct" <<'EOS'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TEST_ROOT/sacct.args"
echo "${SLURM_TIME_FORMAT-unset}" >"$TEST_ROOT/sacct.timefmt"
echo "${TZ-unset}" >"$TEST_ROOT/sacct.tz"
[[ ${FAKE_SACCT_EXIT:-0} == 0 ]] || { echo "sacct: error: slurmdbd: Connection refused" >&2; exit "$FAKE_SACCT_EXIT"; }
[[ -z ${FAKE_SACCT_ROWS:-} ]] || printf '%s\n' "$FAKE_SACCT_ROWS"
EOS
chmod +x "$FAKE_BIN/sacct"
export PATH="$FAKE_BIN:$PATH" TEST_ROOT
unset FAKE_SACCT_ROWS FAKE_SACCT_EXIT

# job <id> <name> <state> <limit> <start> <end> <nodes>: jobcomp/filetxt の 1 行
job() {
  printf 'JobId=%s UserId=tagawa(1000) GroupId=users(100) Name=%s JobState=%s Partition=main TimeLimit=%s StartTime=%s EndTime=%s NodeList=r995 NodeCnt=%s ProcCnt=32 WorkDir=/w ReservationName= Tres=cpu=1,mem=86000M,node=1,billing=1 Account= QOS= WcKey= Cluster=unknown SubmitTime=2026-10-11T12:00:00 EligibleTime=2026-10-11T12:00:00 DerivedExitCode=0:0 ExitCode=0:0 \n' "$@"
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

# 止められうるのは、割り込みを入れた日（2026-10-23）より後に終わったジョブ。ここからのジョブはその後の日付で書く
it "止められていたジョブ（sacct の Suspended が 0 でない）は表に入れず、件数と番号を SUSPENDED で出す"
{
  job 73 openmc-2A COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1
  job 74 openmc-2A COMPLETED 45 2026-11-01T14:24:05 2026-11-01T15:14:32 1
} > "$LOG"
clean=$'73|2026-11-01T13:45:32|00:00:00\n74|2026-11-01T14:24:05|00:00:00'
out=$(FAKE_SACCT_ROWS=$'73|2026-11-01T13:45:32|00:00:00\n74|2026-11-01T14:24:05|00:05:00' "$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_eq 0 $?
assert_contains "$out" "SUSPENDED: 1 (74)"
assert_eq "" "$(grep -E '^ *74 ' <<<"$out")"
assert_contains "$(grep -E '^ *73 ' <<<"$out")" "0.86"

it "--by-name の集計に、止められていたジョブの時間を混ぜない"
out=$(FAKE_SACCT_ROWS=$'73|2026-11-01T13:45:32|00:00:00\n74|2026-11-01T14:24:05|1-00:00:00' "$SCRIPTS_DIR/slurm-history.sh" --by-name "$LOG")
line=$(grep -E '^ *openmc-2A ' <<<"$out")
assert_contains "$line" " 1 "
assert_contains "$line" "0:38:33"
assert_contains "$out" "SUSPENDED: 1 (74)"

it "止められていたジョブが無ければ SUSPENDED: 0 を出す"
out=$(FAKE_SACCT_ROWS=$clean "$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "SUSPENDED: 0"

it "割り込みの後に終わり、表に入れうるジョブ（COMPLETED で NodeCnt が 1 以上）の番号だけをまとめて sacct に渡し、番号の重なる記録も含めて全利用者の開始と Suspended を読む"
{
  job 60 before COMPLETED 45 2026-10-11T13:45:32 2026-10-11T14:24:05 1
  job 73 openmc-2A COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1
  job 77 xlc-cancelled CANCELLED 20 2026-11-01T15:09:25 2026-11-01T15:09:25 0
  job 74 openmc-2A COMPLETED 45 2026-11-01T14:24:05 2026-11-01T15:14:32 1
} > "$LOG"
rm -f "$TEST_ROOT/sacct.args"
FAKE_SACCT_ROWS=$clean "$SCRIPTS_DIR/slurm-history.sh" "$LOG" >/dev/null
args=$(cat "$TEST_ROOT/sacct.args")
assert_contains "$args" $'-j\n73,74\n'
assert_contains "$args" "--allusers"
assert_contains "$args" "--duplicates"
assert_contains "$args" "JobIDRaw,Start,Suspended"

it "割り込みの前に終わったジョブだけなら sacct を呼ばない"
job 60 before COMPLETED 45 2026-10-11T13:45:32 2026-10-11T14:24:05 1 > "$LOG"
rm -f "$TEST_ROOT/sacct.args"
out=$(FAKE_SACCT_EXIT=1 "$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_eq 0 $?
assert_file_missing "$TEST_ROOT/sacct.args"
assert_contains "$out" "SUSPENDED: 0"

it "割り込みの前に終わったジョブは、番号が新しいジョブに再利用されていても表に入れる"
{
  job 150 old COMPLETED 10 2026-10-11T10:00:00 2026-10-11T10:05:00 1
  job 151 new COMPLETED 10 2026-11-01T10:00:00 2026-11-01T10:05:00 1
} > "$LOG"
out=$(FAKE_SACCT_ROWS=$'150|2026-11-02T10:00:00|00:00:00\n151|2026-11-01T10:00:00|00:00:00' "$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_eq 0 $?
assert_contains "$out" "old"

it "番号が再利用されていても、開始の時刻の合う記録で、止められていたかを決める"
{
  job 150 first COMPLETED 10 2026-11-01T10:00:00 2026-11-01T10:05:00 1
  job 150 second COMPLETED 10 2026-11-20T10:00:00 2026-11-20T10:30:00 1
} > "$LOG"
rows=$'150|2026-11-01T10:00:00|00:00:00\n150|2026-11-20T10:00:00|00:20:00'
out=$(FAKE_SACCT_ROWS=$rows "$SCRIPTS_DIR/slurm-history.sh" "$LOG")
assert_contains "$out" "first"
assert_not_contains "$out" "second"
assert_contains "$out" "SUSPENDED: 1 (150)"

{
  job 73 openmc-2A COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1
} > "$LOG"

it "sacct の開始の時刻は、呼ぶ側の SLURM_TIME_FORMAT によらず jobcomp と同じ書式（standard）で読む"
FAKE_SACCT_ROWS=$clean SLURM_TIME_FORMAT=relative "$SCRIPTS_DIR/slurm-history.sh" "$LOG" >/dev/null
assert_eq "standard" "$(cat "$TEST_ROOT/sacct.timefmt")"

it "sacct の開始の時刻は、呼ぶ側の TZ によらず、slurmctld が jobcomp に書くのと同じシステムの時刻帯で読む"
FAKE_SACCT_ROWS=$clean TZ=UTC "$SCRIPTS_DIR/slurm-history.sh" "$LOG" >/dev/null
assert_eq "unset" "$(cat "$TEST_ROOT/sacct.tz")"

it "開始の時刻の欠けた行は、sacct との照合より先に、開始が無いと示して exit 1"
job 73 broken COMPLETED 45 Unknown 2026-11-01T14:24:05 1 > "$LOG"
out=$(FAKE_SACCT_ROWS=$clean "$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "StartTime がありません"

it "割り込みの後に終わったジョブが sacct に無ければ、記録の遅れかもしれないので exit 1"
job 500 late COMPLETED 10 2026-10-23T10:00:00 2026-10-23T10:05:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"
assert_contains "$out" "500"

it "割り込みの前に始まり、後に終わったジョブも、sacct に無ければ exit 1（走っている間に止められうる）"
job 501 long COMPLETED 600 2026-10-22T20:00:00 2026-10-23T06:00:00 1 > "$LOG"
out=$("$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "501"

{
  job 73 openmc-2A COMPLETED 45 2026-11-01T13:45:32 2026-11-01T14:24:05 1
} > "$LOG"

it "sacct にその番号の記録があるのに開始の時刻が合わなければ、止められていたかを決められないので exit 1"
out=$(FAKE_SACCT_ROWS='73|2026-11-01T04:45:32|00:00:00' "$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"
assert_contains "$out" "73"
assert_not_contains "$out" "JOBID"

it "sacct の Suspended が時間の形でなければ、止められていたかを決められないので exit 1"
out=$(FAKE_SACCT_ROWS='73|2026-11-01T13:45:32|' "$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"
assert_not_contains "$out" "JOBID"

it "sacct が失敗したら、止められていたかを確かめられないので exit 1"
out=$(FAKE_SACCT_EXIT=1 "$SCRIPTS_DIR/slurm-history.sh" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR"
assert_not_contains "$out" "JOBID"

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
