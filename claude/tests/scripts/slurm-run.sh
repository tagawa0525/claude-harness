#!/usr/bin/env bash
# claude/scripts/slurm-run.sh（r995 の Slurm に重い計算を 1 本投げて終わりを待つ）のテスト
#
# 実行: bash claude/tests/scripts/slurm-run.sh（まとめて実行: bash claude/tests/scripts.sh）

# 並べ直しで呼ぶ slurm-queue.sh の中身は claude-hooks script slurm-queue（ADR-0101）。ブランチでビルドしたバイナリで確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

# sbatch、scontrol、sacct、hostname の偽物。sbatch は引数を記録し、-o の %x と %j を埋めたログに書き、
# FAKE_JOB_EXIT で終わる（sbatch --wait はジョブの終了コードで終わる）
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/sbatch" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TEST_ROOT/sbatch.args"
# 投げるたびに、偽の squeue の読んだ回数を数え直す
rm -f "$TEST_ROOT/squeue.count" "$TEST_ROOT/sacct.count"
name="" log=""
while (($#)); do
  case "$1" in
    -J) name=$2; shift ;;
    -o) log=$2; shift ;;
  esac
  shift
done
echo "Submitted batch job 42"
log=${log//%x/$name}
log=${log//%j/42}
echo "job output" >"$log"
# FAKE_LOG_READONLY: ジョブが終わったあと、ログに書き足せなくなった状態にする
[[ -z ${FAKE_LOG_READONLY:-} ]] || chmod 444 "$log"
exit "${FAKE_JOB_EXIT:-0}"
EOF
cat >"$FAKE_BIN/scontrol" <<'EOF'
#!/usr/bin/env bash
# show job は終わったジョブの JobState を返す（FAKE_SHOW_EXIT で失敗させられる）
if [[ $1 == show ]]; then
  echo "JobId=42 JobName=x JobState=${FAKE_SCONTROL_STATE:-COMPLETED} Reason=None"
  exit "${FAKE_SHOW_EXIT:-0}"
fi
echo "$*" >>"$TEST_ROOT/scontrol.calls"
# FAKE_TOP_FAILS: 最初のその回数だけ失敗する（列の 1 本が走り始めて scontrol top が拒む状態）
if [[ -n ${FAKE_TOP_FAILS:-} ]] && (($(wc -l <"$TEST_ROOT/scontrol.calls") <= FAKE_TOP_FAILS)); then
  exit 1
fi
exit "${FAKE_TOP_EXIT:-0}"
EOF
# squeue は slurm-queue.sh が読む待ち行列。投げてから n 回目に読んだとき、1 回目は FAKE_SQUEUE_OUT、n 回目は
# FAKE_SQUEUE_OUT<n> を出し（無ければ空。並べ直しが済んだ状態）、FAKE_SQUEUE_EXIT で終わる
cat >"$FAKE_BIN/squeue" <<'EOF'
#!/usr/bin/env bash
# 走っている main のジョブを問い合わせたら（--timed の投げる前の確認）、FAKE_SQUEUE_RUNNING（<番号>|<作業ディレクトリ>）を
# 出し、FAKE_SQUEUE_RUNNING_ERR を標準エラーに出す。読んだ回数に数えない
if [[ " $* " == *" --states=RUNNING,SUSPENDED,CONFIGURING "* ]]; then
  printf '%s\n' "$@" >"$TEST_ROOT/squeue.args.running"
  [[ -z ${FAKE_SQUEUE_RUNNING:-} ]] || printf '%s\n' "$FAKE_SQUEUE_RUNNING"
  [[ -z ${FAKE_SQUEUE_RUNNING_ERR:-} ]] || echo "$FAKE_SQUEUE_RUNNING_ERR" >&2
  exit "${FAKE_SQUEUE_RUNNING_EXIT:-0}"
fi
n=$(($(cat "$TEST_ROOT/squeue.count" 2>/dev/null || echo 0) + 1))
echo "$n" >"$TEST_ROOT/squeue.count"
var=FAKE_SQUEUE_OUT
((n == 1)) || var=FAKE_SQUEUE_OUT$n
[[ -z ${!var:-} ]] || printf '%s\n' "${!var}"
exit "${FAKE_SQUEUE_EXIT:-0}"
EOF
# sacct は終わったジョブの記録（-X -P -n の Elapsed|Timelimit|Partition|Suspended|State）を返す。
# FAKE_SACCT_EMPTY: 最初のその回数は、記録がまだ届いていない（空）。FAKE_SACCT_RUNNING: 同じく RUNNING の記録を返す
cat >"$FAKE_BIN/sacct" <<'EOF2'
#!/usr/bin/env bash
n=$(($(cat "$TEST_ROOT/sacct.count" 2>/dev/null || echo 0) + 1))
echo "$n" >"$TEST_ROOT/sacct.count"
[[ -z ${FAKE_SACCT_EXIT:-} ]] || exit "$FAKE_SACCT_EXIT"
((n > ${FAKE_SACCT_EMPTY:-0})) || exit 0
part=main
grep -qx -- '--partition=timed' "$TEST_ROOT/sbatch.args" && part=timed
state=${FAKE_STATE:-COMPLETED}
((n > ${FAKE_SACCT_EMPTY:-0} + ${FAKE_SACCT_RUNNING:-0})) || state=${FAKE_SACCT_WAIT_STATE:-RUNNING}
echo "${FAKE_SACCT_ELAPSED:-00:12:34}|${FAKE_SACCT_LIMIT:-00:30:00}|${FAKE_SACCT_PARTITION-$part}|${FAKE_SACCT_SUSPENDED-00:00:00}|$state"
EOF2
cat >"$FAKE_BIN/hostname" <<'EOF'
#!/usr/bin/env bash
echo "${FAKE_HOST:-r995}"
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH" TEST_ROOT
export SLURM_RUN_SACCT_INTERVAL=0
# テストを投げるセッションの ID と、入口の置き換えを持ち込まない
unset CLAUDE_CODE_SESSION_ID SLURM_RUN_NFS_ENTRY
# r995 の NFS の入口（既定は /mnt/r995/github）を、テストの中に置き換える
NFS_ENTRY="$TEST_ROOT/mnt/r995/github"

RUN="$SCRIPTS_DIR/slurm-run.sh"
# 目的と区分（必須）。目的と区分を確かめるテストのほかは、この組で投げる
PURPOSE=(--class trial --ref '#1' --purpose '試験の目的')
mkdir -p "$HOME/github/proj" "$NFS_ENTRY/proj"

it "slurm-run: 排他・待機・環境の引き継ぎ・名前・ログ・見積もり・連絡先を付けてコマンドを投げる"
cd "$HOME/github/proj" || exit 1
git init -q -b feat/vera-2g .
out=$("$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'cargo run --release -- p2/assembly-2G.toml')
assert_eq 0 $?
args=$(cat "$TEST_ROOT/sbatch.args")
assert_contains "$args" "--exclusive"
assert_contains "$args" "--wait"
assert_contains "$args" "--export=ALL"
assert_contains "$args" $'-J\nxlc-2G'
assert_contains "$args" $'-o\n'"$HOME/github/slurm-logs/%x-%j.out"
assert_contains "$args" $'--wrap\ncargo run --release -- p2/assembly-2G.toml'
assert_contains "$args" $'--time\n30'
assert_contains "$args" $'--comment\ntrial | #1 | 試験の目的 | feat/vera-2g '"$HOME/github/proj"

it "slurm-run: ジョブの番号、ログの場所、ログの末尾、終了コードを出す"
assert_contains "$out" "JOB: 42"
assert_contains "$out" "LOG: $HOME/github/slurm-logs/xlc-2G-42.out"
assert_contains "$out" "job output"
assert_contains "$out" "EXIT: 0"

it "slurm-run: 終わったジョブの番号、実際の時間、見積もりを 1 行に出し、ログの末尾にも書く"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-2G-42.out")" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"

it "slurm-run: ジョブが失敗しても、実際の時間と見積もりを出す"
out=$(FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'false')
assert_eq 3 $?
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"

it "slurm-run: ログに書き足せなくても、警告してジョブの終了コードで終わる"
out=$(FAKE_LOG_READONLY=1 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-ro 'false' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_contains "$out" "EXIT: 3"

it "slurm-run: -c <コア数> を付けると、--exclusive を付けず --cpus-per-task を渡し、連絡先に印を付ける"
cd "$HOME/github/proj" || exit 1
"$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G-shared 'true' >/dev/null
args=$(cat "$TEST_ROOT/sbatch.args")
assert_not_contains "$args" "--exclusive"
assert_contains "$args" "--cpus-per-task=8"
assert_contains "$args" "--wait"
assert_contains "$args" "shared-cpus=8"

it "slurm-run: -c を付けたジョブの TIME 行とログの末尾に、時間は当てにならない印を付ける"
out=$("$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G-shared 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 shared (-c 8): 時間は当てにならない"
assert_contains "$(cat "$HOME/github/slurm-logs/openmc-2G-shared-42.out")" "shared (-c 8): 時間は当てにならない"

it "slurm-run: -c を付けなければ、排他で投げ、TIME 行に印を付けない"
out=$("$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "--exclusive"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "--cpus-per-task"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "shared-cpus"
assert_not_contains "$out" "shared"

it "slurm-run: --timed を付けると、排他でパーティション timed に投げる"
cd "$HOME/github/proj" || exit 1
"$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' >/dev/null
args=$(cat "$TEST_ROOT/sbatch.args")
assert_contains "$args" "--partition=timed"
assert_contains "$args" "--exclusive"

it "slurm-run: --timed は -c と組み合わせられず、投げずにエラー"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" "${PURPOSE[@]}" --timed -c 4 -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_file_missing "$TEST_ROOT/sbatch.args"

# パーティションの名前（main、timed）と NFS の入口は、クラスタの構成（nixfiles の modules/slurm/）との約束。構成と同じことは、
# 構成を持つ nixfiles の側のテストが確かめる
it "slurm-run: --timed を付けなければ、既定のパーティション（main）を明示して投げる（SBATCH_PARTITION に左右されない）"
default_part=main
SBATCH_PARTITION=timed "$RUN" "${PURPOSE[@]}" -t 5 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "--partition=$default_part"
SBATCH_PARTITION=timed "$RUN" "${PURPOSE[@]}" -c 4 -t 5 xlc-2G-shared 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "--partition=$default_part"

it "slurm-run: --timed でないジョブの TIME 行とログの末尾には、止められていなくても割り込まれうる印を付ける"
out=$("$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 preemptible: 割り込まれうるので時間は記録に使わない（記録するなら排他の --timed）"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-2G-42.out")" "preemptible: 割り込まれうるので時間は記録に使わない"

it "slurm-run: --timed のジョブの TIME 行には印を付けない"
out=$("$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_not_contains "$out" "preemptible"
assert_not_contains "$out" "当てにならない"

it "slurm-run: 割り込まれうる印は、flag でなく、ジョブが実際に走ったパーティション（scontrol の Partition）で決める"
out=$(FAKE_SACCT_PARTITION=main "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
out=$(FAKE_SACCT_PARTITION=timed "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_not_contains "$out" "preemptible"

it "slurm-run: ジョブが止められていたら（SuspendTime がある）、TIME 行とログの末尾に、時間は当てにならない印を付ける"
out=$(FAKE_SACCT_SUSPENDED=00:10:00 "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 suspended: 止められていたので時間は当てにならない"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-bench-42.out")" "suspended: 止められていたので時間は当てにならない"

it "slurm-run: 止められていない（SuspendTime=None）ジョブの TIME 行には、止められた印を付けない"
out=$("$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_not_contains "$out" "suspended"

it "slurm-run: 割り込まれうるジョブが止められていたら、両方の印を付ける"
out=$(FAKE_SACCT_SUSPENDED=00:10:00 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
assert_contains "$out" "suspended: 止められていたので時間は当てにならない"

it "slurm-run: 排他でなく、止められてもいたら、すべての印を付ける"
out=$(FAKE_SACCT_SUSPENDED=00:10:00 "$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G-shared 'true')
assert_contains "$out" "shared (-c 8): 時間は当てにならない"
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
assert_contains "$out" "suspended: 止められていたので時間は当てにならない"

it "slurm-run: -c が正の整数でなければ、投げずにエラー"
for bad in 0 -1 abc 1.5 ""; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${PURPOSE[@]}" -c "$bad" -t 30 openmc-2G-shared 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR:"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: -c にコア数がなければ使い方を出してエラー"
out=$("$RUN" "${PURPOSE[@]}" -t 30 -c 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

it "slurm-run: ジョブが失敗したら、その終了コードで終わる"
out=$(FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'false')
assert_eq 3 $?
assert_contains "$out" "EXIT: 3"

Q="feat/x /p"
it "slurm-run: main に投げたら、待っているジョブを区分の順に並べ直す（投げたジョブは自分の区分の末尾）"
rm -f "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "20|main|t|PENDING|10010|10:00|N/A|trial | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|record | #1 | p | $Q")" \
  "$RUN" --class record --ref '#1' --purpose '本番' -t 5 xlc-rec 'true' >/dev/null
assert_eq "top 42,20" "$(cat "$TEST_ROOT/scontrol.calls")"

it "slurm-run: --top を付けると、投げたジョブを自分の区分の先頭に置き、上の区分のジョブは追い越さない"
rm -f "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "21|main|r|PENDING|10030|10:00|N/A|record | #1 | p | $Q" \
  "20|main|t|PENDING|10020|10:00|N/A|trial | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|trial | #1 | p | $Q")" \
  "$RUN" "${PURPOSE[@]}" --top -t 5 xlc-0005-1B 'true' >/dev/null
assert_eq "top 21,42,20" "$(cat "$TEST_ROOT/scontrol.calls")"
rm -f "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "21|main|r|PENDING|10030|10:00|N/A|record | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|trial | #1 | p | $Q")" \
  "$RUN" "${PURPOSE[@]}" --top -t 5 xlc-0005-1B 'true' >/dev/null
assert_file_missing "$TEST_ROOT/scontrol.calls"

it "slurm-run: 待っているジョブが区分の順に並んでいれば、順番に触らない"
rm -f "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "21|main|r|PENDING|10030|10:00|N/A|record | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|trial | #1 | p | $Q")" \
  "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_file_missing "$TEST_ROOT/scontrol.calls"

it "slurm-run: パーティション timed では区分で並べ直さず、--top のときだけ投げたジョブを先頭に移す"
rm -f "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "21|timed|bench|PENDING|10030|60:00|N/A|timed | #1 | p | $Q" \
  "42|timed|quick|PENDING|10000|5:00|N/A|decision | #1 | p | $Q")" \
  "$RUN" --class decision --ref '#1' --purpose '急ぐ確認' --timed -t 5 xlc-quick 'true' >/dev/null
assert_file_missing "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "21|timed|bench|PENDING|10030|60:00|N/A|timed | #1 | p | $Q" \
  "42|timed|quick|PENDING|10000|5:00|N/A|decision | #1 | p | $Q")" \
  "$RUN" --class decision --ref '#1' --purpose '急ぐ確認' --timed --top -t 5 xlc-quick 'true' >/dev/null
assert_eq "top 42" "$(cat "$TEST_ROOT/scontrol.calls")"

it "slurm-run: 並べ替えが拒まれ続けても、警告してジョブの終わりまで待つ"
rm -f "$TEST_ROOT/scontrol.calls"
unsorted="$(printf '%s\n' \
  "20|main|t|PENDING|10010|10:00|N/A|trial | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|trial | #1 | p | $Q")"
out=$(FAKE_TOP_EXIT=1 FAKE_JOB_EXIT=3 FAKE_SQUEUE_OUT=$unsorted FAKE_SQUEUE_OUT2=$unsorted FAKE_SQUEUE_OUT3=$unsorted FAKE_SQUEUE_OUT4=$unsorted \
  "$RUN" "${PURPOSE[@]}" --top -t 5 xlc-0005-1B 'true' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "EXIT: 3"

it "slurm-run: 並べ替えが拒まれても、待ち行列を読み直して並べ直す"
rm -f "$TEST_ROOT/scontrol.calls"
out=$(FAKE_TOP_FAILS=1 FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "20|main|t|PENDING|10020|10:00|N/A|trial | #1 | p | $Q" \
  "21|main|r|PENDING|10010|10:00|N/A|record | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|record | #1 | p | $Q")" FAKE_SQUEUE_OUT2="$(printf '%s\n' \
  "20|main|t|RUNNING|10020|10:00|N/A|trial | #1 | p | $Q" \
  "23|main|t2|PENDING|10015|10:00|N/A|trial | #1 | p | $Q" \
  "21|main|r|PENDING|10010|10:00|N/A|record | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|record | #1 | p | $Q")" \
  "$RUN" --class record --ref '#1' --purpose '本番' -t 5 xlc-rec 'true' 2>&1)
assert_eq $'top 21,42,20\ntop 21,42,23' "$(cat "$TEST_ROOT/scontrol.calls")"
assert_not_contains "$out" "WARN:"

it "slurm-run: 並べ替えが成功しても読み直し、同時に投げたジョブが列を上書きしていれば並べ直す"
# A（42）が読んだ列 42,20 を渡した後、同時に投げた B（43）の列が先に通っていて、B が trial の後ろに残った状態
rm -f "$TEST_ROOT/scontrol.calls"
out=$(FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "20|main|t|PENDING|10010|10:00|N/A|trial | #1 | p | $Q" \
  "42|main|a|PENDING|10000|10:00|N/A|record | #1 | p | $Q")" FAKE_SQUEUE_OUT2="$(printf '%s\n' \
  "42|main|a|PENDING|10010|10:00|N/A|record | #1 | p | $Q" \
  "20|main|t|PENDING|10009|10:00|N/A|trial | #1 | p | $Q" \
  "43|main|b|PENDING|10000|10:00|N/A|record | #1 | p | $Q")" \
  "$RUN" --class record --ref '#1' --purpose '本番' -t 5 xlc-rec 'true' 2>&1)
assert_eq $'top 42,20\ntop 42,43,20' "$(cat "$TEST_ROOT/scontrol.calls")"
assert_not_contains "$out" "WARN:"

it "slurm-run: 3 回並べ直しても区分の順にならなければ、警告してジョブの終わりまで待つ"
rm -f "$TEST_ROOT/scontrol.calls"
unsorted="$(printf '%s\n' \
  "20|main|t|PENDING|10020|10:00|N/A|trial | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|record | #1 | p | $Q")"
out=$(FAKE_JOB_EXIT=3 FAKE_SQUEUE_OUT=$unsorted FAKE_SQUEUE_OUT2=$unsorted FAKE_SQUEUE_OUT3=$unsorted FAKE_SQUEUE_OUT4=$unsorted \
  "$RUN" --class record --ref '#1' --purpose '本番' -t 5 xlc-rec 'true' 2>&1)
assert_eq 3 $?
assert_eq 3 "$(wc -l <"$TEST_ROOT/scontrol.calls")"
assert_contains "$out" "WARN:"

it "slurm-run: 区分の無いジョブだけが前で待っていても、--top のジョブをその前に出す"
rm -f "$TEST_ROOT/scontrol.calls"
FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "90|main|old|PENDING|10060|10:00|N/A|$Q" \
  "42|main|new|PENDING|10000|10:00|N/A|record | #1 | p | $Q")" \
  "$RUN" --class record --ref '#1' --purpose '本番' --top -t 5 xlc-rec 'true' >/dev/null
assert_eq "top 42" "$(cat "$TEST_ROOT/scontrol.calls")"

it "slurm-run: 待ち行列を読めなくても、警告して並べ直さずにジョブの終わりまで待つ"
rm -f "$TEST_ROOT/scontrol.calls"
out=$(FAKE_SQUEUE_EXIT=2 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" --top -t 5 xlc-0005-1B 'true' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "EXIT: 3"
assert_file_missing "$TEST_ROOT/scontrol.calls"

it "slurm-run: git の外では連絡先に場所だけを書く"
mkdir -p "$HOME/github/plain" && cd "$HOME/github/plain" || exit 1
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\ntrial | #1 | 試験の目的 | '"$HOME/github/plain"

it "slurm-run: Claude Code から投げたら、連絡先にセッションの ID を足す"
cd "$HOME/github/proj" || exit 1
CLAUDE_CODE_SESSION_ID=abc-123 "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\ntrial | #1 | 試験の目的 | feat/vera-2g '"$HOME/github/proj claude=abc-123"

it "slurm-run: git の中では、連絡先の場所の後に投げた時点の HEAD の短い hash を commit= で足す"
mkdir -p "$HOME/github/committed" && cd "$HOME/github/committed" || exit 1
git init -q -b feat/commit .
echo a >input.toml
git add input.toml && git commit -q -m "feat: seed"
head=$(git rev-parse --short HEAD)
CLAUDE_CODE_SESSION_ID=abc-123 "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\ntrial | #1 | 試験の目的 | feat/commit '"$HOME/github/committed commit=$head claude=abc-123"

it "slurm-run: git が無視するファイルだけがあるときは、未コミットの印を付けない"
echo 'trials/' >.git/info/exclude
mkdir -p trials && echo out >trials/run.log
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head-dirty"

it "slurm-run: まだ追加していない、無視されない新しいファイルがあれば、hash に -dirty を付ける"
echo 'fn main() {}' >new_source.rs
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head-dirty"
rm new_source.rs

it "slurm-run: status.showUntrackedFiles が no でも、追加していないファイルで -dirty を付ける"
git config status.showUntrackedFiles no
echo 'fn main() {}' >new_source.rs
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head-dirty"
rm new_source.rs
git config --unset status.showUntrackedFiles

it "slurm-run: diff.ignoreSubmodules が all でも、サブモジュールの新しいコミットで -dirty を付ける"
git init -q -b main "$TEST_ROOT/subsrc"
git -C "$TEST_ROOT/subsrc" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
git -c protocol.file.allow=always submodule add -q "$TEST_ROOT/subsrc" sub
git commit -q -m "feat: add sub"
head=$(git rev-parse --short HEAD)
git config diff.ignoreSubmodules all
git -C sub -c user.name=t -c user.email=t@t commit -q --allow-empty -m next
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head-dirty"
git config --unset diff.ignoreSubmodules
git submodule deinit -q -f sub
git rm -q -f sub
git commit -q -m "chore: drop sub"
head=$(git rev-parse --short HEAD)

it "slurm-run: 追跡しているファイルに未コミットの変更があれば、hash に -dirty を付ける"
echo b >input.toml
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head-dirty"

it "slurm-run: ステージしただけの変更も未コミットの変更として -dirty を付ける"
git add input.toml
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit=$head-dirty"
git checkout -q HEAD -- input.toml

it "slurm-run: HEAD が切り離されていても、投げた版のコミットを足す"
git checkout -q --detach
"$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\ntrial | #1 | 試験の目的 | '"$HOME/github/committed commit=$head"
git checkout -q feat/commit

it "slurm-run: git の外とコミットの無いリポジトリでは commit= を足さない"
for dir in "$HOME/github/plain" "$HOME/github/proj"; do
  cd "$dir" || exit 1
  "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' >/dev/null
  assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit="
done

it "slurm-run: git の中で版を調べられなくても、警告して commit= を付けずに投げる"
cd "$HOME/github/committed" || exit 1
cp .git/index "$TEST_ROOT/index.bak"
echo garbage >.git/index
out=$("$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN: commit= を付けません"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit="
cp "$TEST_ROOT/index.bak" .git/index

it "slurm-run: bare リポジトリや git の外でない失敗では、git の外と区別して警告して投げる"
git init -q --bare "$HOME/github/bare.git"
cd "$HOME/github/bare.git" || exit 1
out=$("$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN: commit= を付けません"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "commit="
cd "$HOME/github/plain" || exit 1
out=$("$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' 2>&1)
assert_not_contains "$out" "WARN: commit="
cd "$HOME/github/committed" || exit 1

it "slurm-run: r995 以外では NFS の入口の下で投げ、ログは NFS の側に作る"
rm -rf "$HOME/github/slurm-logs" "$NFS_ENTRY/slurm-logs"
cd "$NFS_ENTRY/proj" || exit 1
out=$(SLURM_RUN_NFS_ENTRY=$NFS_ENTRY FAKE_HOST=t14g4 "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true')
assert_eq 0 $?
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'-o\n'"$HOME/github/slurm-logs/%x-%j.out"
assert_contains "$out" "LOG: $NFS_ENTRY/slurm-logs/xlc-2G-42.out"
assert_file_exists "$NFS_ENTRY/slurm-logs"

it "slurm-run: r995 以外で NFS の入口の外から投げたら、投げずに既定の入口 /mnt/r995/github を示してエラー"
rm -f "$TEST_ROOT/sbatch.args"
cd "$HOME/github/proj" || exit 1
out=$(FAKE_HOST=t14g4 "$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_contains "$out" "/mnt/r995/github"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: 既定の NFS の入口は /mnt/r995/github"
assert_eq "/mnt/r995/github" "$(grep -o 'SLURM_RUN_NFS_ENTRY:-[^}]*' "$RUN" | cut -d- -f2-)"

it "slurm-run: 知らないオプション（消した --urgent など）は、名前と読まずに投げずにエラー"
cd "$HOME/github/proj" || exit 1
for opt in --urgent -x; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${PURPOSE[@]}" -t 5 "$opt" xlc-quick 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR:"
  assert_contains "$out" "$opt"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: 名前がファイル名に使えない文字を含んだらエラー"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" "${PURPOSE[@]}" -t 1 'xlc 2G/a' 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "slurm-run: 名前かコマンドがなければ使い方を出してエラー"
out=$("$RUN" "${PURPOSE[@]}" -t 1 xlc-2G 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

it "slurm-run: 見積もりの時間がなければ使い方を出してエラー"
out=$("$RUN" "${PURPOSE[@]}" xlc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

it "slurm-run: 区分、参照、目的をジョブのコメントの先頭に「区分 | 参照 | 目的 | 連絡先」の形で入れる"
cd "$HOME/github/proj" || exit 1
"$RUN" --class decision --ref ADR-0175 --purpose '平衡キセノンで振動が消えるかで、修正方法を決める' -t 30 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\ndecision | ADR-0175 | 平衡キセノンで振動が消えるかで、修正方法を決める | feat/vera-2g '"$HOME/github/proj"

it "slurm-run: TIME 行とログの末尾に、区分、参照、目的を出す"
out=$("$RUN" --class record --ref '#0200' --purpose '記録に載せる小さい炉心の燃焼' -t 30 xlc-2G 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_contains "$out" "class=record ref=#0200 purpose=記録に載せる小さい炉心の燃焼"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-2G-42.out")" "class=record ref=#0200 purpose=記録に載せる小さい炉心の燃焼"

it "slurm-run: 区分、参照、目的のどれかが無ければ、投げずに使い方を出してエラー"
for missing in class ref purpose; do
  given=()
  [[ $missing == class ]] || given+=(--class trial)
  [[ $missing == ref ]] || given+=(--ref '#1')
  [[ $missing == purpose ]] || given+=(--purpose '試験の目的')
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${given[@]}" -t 5 xlc-2G 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR: --$missing が無い"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: 知らない区分は、投げずに区分の一覧を示してエラー"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" --class urgent --ref '#1' --purpose '試験の目的' -t 5 xlc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_contains "$out" "timed record decision trial"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: 参照は #番号、ADR-番号、それらのカンマ区切り、または「- <理由>」を受け付ける"
for ref in '#0200' 'ADR-0175' '#0200,ADR-0175' '- 環境の確認で、記録の番号が無い'; do
  rm -f "$TEST_ROOT/sbatch.args"
  "$RUN" --class trial --ref "$ref" --purpose '試験の目的' -t 5 xlc-2G 'true' >/dev/null 2>&1
  assert_eq 0 $?
  assert_contains "$(cat "$TEST_ROOT/sbatch.args" 2>/dev/null)" "trial | $ref | 試験の目的 | "
done

it "slurm-run: 参照の形でなければ（番号の無い #、理由の無い -、ただの語）、投げずにエラー"
for ref in '#' 'ADR-' '-' '- ' '0200' 'issue 200' '#0200,' '#02a0' '#0200 | x' $'- 理由\n2 行目'; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" --class trial --ref "$ref" --purpose '試験の目的' -t 5 xlc-2G 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR:"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: 目的が空、空白だけ、改行か | を含むなら、投げずにエラー"
for purpose in '' '   ' $'1 行目\n2 行目' 'a | b'; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" --class trial --ref '#1' --purpose "$purpose" -t 5 xlc-2G 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR:"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: 区分 timed は、--timed を付けなくてもパーティション timed に排他で投げる"
"$RUN" --class timed --ref '#1' --purpose '高速化の測定' -t 5 xlc-bench 'true' >/dev/null
args=$(cat "$TEST_ROOT/sbatch.args")
assert_contains "$args" "--partition=timed"
assert_contains "$args" "--exclusive"

it "slurm-run: 区分 timed は -c と組めず、投げずにエラー"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" --class timed --ref '#1' --purpose '高速化の測定' -c 4 -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: ほかの区分に --timed を付けると（数分の急ぐ計算）、区分はそのままでパーティション timed に投げる"
"$RUN" --class decision --ref '#1' --purpose '急ぐ確認' --timed -t 5 xlc-quick 'true' >/dev/null
args=$(cat "$TEST_ROOT/sbatch.args")
assert_contains "$args" "--partition=timed"
assert_contains "$args" $'--comment\ndecision | #1 | 急ぐ確認 | '

cd "$HOME/github/proj" || exit 1

it "slurm-run: scancel で止めたジョブ（sbatch は 0、State は CANCELLED）は、0 以外で終わり、State を出す"
out=$(FAKE_STATE="CANCELLED by 1000" "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'sleep 999')
assert_ne 0 $?
assert_contains "$out" "state=CANCELLED"
assert_contains "$out" "EXIT: 1 (State=CANCELLED)"

it "slurm-run: 取り消したジョブの後ろに && でつないだコマンドは走らない"
ran=no
FAKE_STATE=CANCELLED "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'sleep 999' >/dev/null && ran=yes
assert_eq no "$ran"

it "slurm-run: 時間切れ（TIMEOUT）は 0 以外で終わる"
FAKE_STATE=TIMEOUT "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' >/dev/null
assert_ne 0 $?

it "slurm-run: 失敗（FAILED）はジョブの終了コードのまま、State を出す"
out=$(FAKE_STATE=FAILED FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'false')
assert_eq 3 $?
assert_contains "$out" "state=FAILED"
assert_contains "$out" "EXIT: 3 (State=FAILED)"

it "slurm-run: 完了（COMPLETED）は 0 で終わり、State を出す"
out=$("$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_eq 0 $?
assert_contains "$out" "state=COMPLETED"
assert_not_contains "$out" "(State="

it "slurm-run: 時間は scontrol でなく sacct から読む"
out=$(FAKE_SACCT_ELAPSED=01:02:03 FAKE_SACCT_LIMIT=02:00:00 "$RUN" "${PURPOSE[@]}" -t 120 xlc-2G 'true' 2>&1)
assert_contains "$out" "TIME: job=42 elapsed=01:02:03 estimate=02:00:00"
assert_not_contains "$out" "WARN:"

it "slurm-run: sacct に記録が届くまで待ち、届いたら読む"
out=$(FAKE_SACCT_EMPTY=2 FAKE_SACCT_RUNNING=1 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_contains "$out" "TIME: job=42 elapsed=00:12:34"
assert_eq 4 "$(cat "$TEST_ROOT/sacct.count")"

it "slurm-run: 記録が届かなければ、警告して TIME 行を出さず、ジョブの終了コードで終わる"
out=$(SLURM_RUN_SACCT_TRIES=3 FAKE_SACCT_EMPTY=99 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'false' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_not_contains "$out" "TIME:"
assert_eq 3 "$(cat "$TEST_ROOT/sacct.count")"

it "slurm-run: sacct が読めなければ、警告して TIME 行を出さず、scontrol の JobState が COMPLETED なら 0 で終わる"
out=$(SLURM_RUN_SACCT_TRIES=2 FAKE_SACCT_EXIT=1 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN:"
assert_not_contains "$out" "TIME:"

it "slurm-run: sacct の Partition と Suspended から、割り込まれうる印と止められた印を決める"
out=$(FAKE_SACCT_PARTITION=main FAKE_SACCT_SUSPENDED=00:05:00 "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
assert_contains "$out" "suspended: 止められていたので時間は当てにならない"
out=$(FAKE_SACCT_PARTITION=timed "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_not_contains "$out" "preemptible"
assert_not_contains "$out" "suspended"

it "slurm-run: sacct の Partition か Suspended が空なら、確かめられない印を付ける"
out=$(FAKE_SACCT_PARTITION='' FAKE_SACCT_SUSPENDED='' "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_contains "$out" "partition=unknown"
assert_contains "$out" "suspended=unknown"

cd "$HOME/github/proj" || exit 1

it "slurm-run: sacct に記録が届かなくても、scontrol の JobState で終わりの State を確かめ、CANCELLED なら 0 以外で終わる"
out=$(SLURM_RUN_SACCT_TRIES=2 FAKE_SACCT_EMPTY=99 FAKE_SCONTROL_STATE=CANCELLED "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'sleep 999' 2>&1)
assert_ne 0 $?
assert_contains "$out" "EXIT: 1 (State=CANCELLED)"
assert_not_contains "$out" "TIME:"

it "slurm-run: sacct に記録が届かず scontrol が COMPLETED なら、警告して 0 で終わる"
out=$(SLURM_RUN_SACCT_TRIES=2 FAKE_SACCT_EMPTY=99 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN:"
assert_not_contains "$out" "TIME:"

it "slurm-run: sacct も scontrol も終わりの State を返さなければ、成功扱いにせず 0 以外で終わる"
out=$(SLURM_RUN_SACCT_TRIES=2 FAKE_SACCT_EXIT=1 FAKE_SHOW_EXIT=1 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_ne 0 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "EXIT: 1 (State=unknown)"
out=$(SLURM_RUN_SACCT_TRIES=2 FAKE_SACCT_EXIT=1 FAKE_SCONTROL_STATE=RUNNING "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_ne 0 $?

it "slurm-run: State を確かめられなくても、ジョブが 0 以外で終わっていれば、その終了コードで終わる"
out=$(SLURM_RUN_SACCT_TRIES=2 FAKE_SACCT_EXIT=1 FAKE_SHOW_EXIT=1 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'false' 2>&1)
assert_eq 3 $?

it "slurm-run: CONFIGURING など終わりでない State は、終わりの State が届くまで待つ"
out=$(FAKE_SACCT_WAIT_STATE=CONFIGURING FAKE_SACCT_RUNNING=2 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true' 2>&1)
assert_contains "$out" "TIME: job=42 elapsed=00:12:34"
assert_contains "$out" "state=COMPLETED"
assert_eq 3 "$(cat "$TEST_ROOT/sacct.count")"

it "slurm-run: --ref の値が無ければ（囲まない # から後ろがシェルの注釈になった）、投げずに引用符で囲む形を示してエラー"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" --class trial --ref 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: --ref の値が無い"
assert_contains "$out" "--ref '#0200'"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: -t が分の数か H:MM:SS（日-時の形も）でなければ（25m、1h、sbatch が分:秒と読む 1:30 など）、投げずに分の数か H:MM:SS の形を示してエラー"
for bad in 25m 1h 1.5 abc 1:30 30:00; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${PURPOSE[@]}" -t "$bad" xlc-2G 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR: -t は分の数（-t 90）か H:MM:SS（-t 1:30:00）"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: -t は分、時:分:秒、日-時、日-時:分、日-時:分:秒を受け付ける"
for good in 90 1:30:00 1-00 1-12:00 2-12:00:00; do
  rm -f "$TEST_ROOT/sbatch.args"
  "$RUN" "${PURPOSE[@]}" -t "$good" xlc-2G 'true' >/dev/null 2>&1
  assert_eq 0 $?
  assert_contains "$(cat "$TEST_ROOT/sbatch.args" 2>/dev/null)" $'--time\n'"$good"
done

it "slurm-run: コマンドに /usr/bin/time があれば（r995 に無く、ジョブが 0 秒で失敗する）、投げずに時間の取り方を示してエラー"
for cmd in '/usr/bin/time -v xs run.toml' 'cd sub && /usr/bin/time xs run.toml' 'env A=1 /usr/bin/time -f %e xs'; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${PURPOSE[@]}" -t 5 xlc-2G "$cmd" 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR: /usr/bin/time"
  assert_contains "$out" "sacct -j <番号>.batch -o Elapsed,MaxRSS"
  assert_contains "$out" 'command time -o <ファイル> -f "%e s %M KB"'
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: /usr/bin/time を含まないコマンド（command time、time の予約語、似た名前）は投げる"
for cmd in 'command time -o t.txt xs' 'time xs' '/usr/bin/timeout 5 xs' 'ls /usr/bin/time-tools'; do
  rm -f "$TEST_ROOT/sbatch.args"
  "$RUN" "${PURPOSE[@]}" -t 5 xlc-2G "$cmd" >/dev/null 2>&1
  assert_eq 0 $?
  assert_file_exists "$TEST_ROOT/sbatch.args"
done

it "slurm-run: -c のジョブの名前の末尾が -shared でなければ、投げずに付ける形を示してエラー"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: -c のジョブの名前は末尾に -shared を付ける（openmc-2G-shared）"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: --timed と -c を組んだときのエラーは、どちらを外すかを示す"
out=$("$RUN" "${PURPOSE[@]}" --timed -c 4 -t 5 xlc-bench-shared 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "時間を記録するなら -c を外し、記録しないなら --timed を外す"

it "slurm-run: --timed のジョブは、走っている main のジョブと同じ worktree からは投げずに、別の worktree か終わりを待つ形を示してエラー"
cd "$HOME/github/committed" || exit 1
mkdir -p sub
for path in "$HOME/github/committed" "$HOME/github/committed/sub"; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$(FAKE_SQUEUE_RUNNING="700|$path" \
    "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR: 走っている main のジョブ 700 が同じ worktree"
  assert_contains "$out" "NEXT: 別の worktree（git worktree add）から投げるか、ジョブ 700 が終わってから投げる"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done
out=$(cd sub && FAKE_SQUEUE_RUNNING="700|$HOME/github/committed" \
  "$RUN" "${PURPOSE[@]}" --class timed -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: 走っている main のジョブ 700"

it "slurm-run: --timed のジョブは、走っている main のジョブが別の worktree なら投げる。timed でないジョブは確かめない"
cd "$HOME/github/committed" || exit 1
rm -f "$TEST_ROOT/sbatch.args"
FAKE_SQUEUE_RUNNING="701|$HOME/github/proj" \
  "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' >/dev/null 2>&1
assert_eq 0 $?
assert_file_exists "$TEST_ROOT/sbatch.args"
rm -f "$TEST_ROOT/sbatch.args"
FAKE_SQUEUE_RUNNING="700|$HOME/github/committed" \
  "$RUN" "${PURPOSE[@]}" -t 5 xlc-2G 'true' >/dev/null 2>&1
assert_eq 0 $?
assert_file_exists "$TEST_ROOT/sbatch.args"

it "slurm-run: 走っている main のジョブを読めなければ、警告して --timed のジョブを投げる"
rm -f "$TEST_ROOT/sbatch.args"
out=$(FAKE_SQUEUE_RUNNING_EXIT=1 "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
assert_eq 0 $?
assert_contains "$out" "WARN: 走っている main のジョブを読めず"
assert_file_exists "$TEST_ROOT/sbatch.args"

it "slurm-run: 投げた直後に、ジョブの走る場所を NEXT: で示す"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" "${PURPOSE[@]}" -t 5 xlc-2G 'true' 2>&1)
assert_contains "$out" "NEXT: ジョブは $HOME/github/proj で走る。意図した worktree でなければ scancel 42 で取り消し"
assert_eq "LOG:" "$(grep -A1 '^JOB:' <<<"$out" | sed -n 2p | cut -d' ' -f1)"

it "slurm-run: TIME 行に印があれば、その時間を記録に使わないことと測り直し方を NEXT: で示す"
cd "$HOME/github/proj" || exit 1
for opts in "-t 5" "-c 4 -t 5" "--timed -t 5"; do
  name=xlc-2G
  [[ $opts != -c* ]] || name=xlc-2G-shared
  suspended=00:00:00
  [[ $opts != --timed* ]] || suspended=00:01:00
  # shellcheck disable=SC2086 # オプションを分けて渡す
  out=$(FAKE_SACCT_SUSPENDED=$suspended "$RUN" "${PURPOSE[@]}" $opts "$name" 'true' 2>&1)
  assert_contains "$out" "NEXT: TIME 行に印があるので、この時間（elapsed も、プログラムが自分で測った時間も）は記録に使わない"
  # 時間を記録しない main のジョブにまで、timed で投げ直させない
  assert_contains "$out" "時間を記録しない計算は投げ直さない"
done

it "slurm-run: 印の無い TIME 行の後には、記録に使わない NEXT: を出さない"
out=$("$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
assert_not_contains "$out" "NEXT: TIME 行"

it "slurm-run: EXIT: の前に、ジョブの番号と名前と成否を 1 行にした RESULT: を出す（wait で待った複数のジョブを見分ける）"
out=$("$RUN" "${PURPOSE[@]}" -t 5 xlc-2G 'true' 2>&1)
assert_contains "$out" "RESULT: job=42 name=xlc-2G exit=0 state=COMPLETED"
assert_eq "EXIT: 0" "$(grep -A1 '^RESULT:' <<<"$out" | sed -n 2p)"
out=$(FAKE_STATE=FAILED FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 5 xlc-2G 'false' 2>&1)
assert_contains "$out" "RESULT: job=42 name=xlc-2G exit=3 state=FAILED"

it "slurm-run: r995 と NFS の入口で書き方の違う同じ worktree も、同じとみなして --timed のジョブを止める"
# r995 以外から NFS の入口の下で投げ、走っているジョブは r995 の上のパスで投げられている
cd "$NFS_ENTRY/proj" || exit 1
rm -f "$TEST_ROOT/sbatch.args"
out=$(SLURM_RUN_NFS_ENTRY=$NFS_ENTRY FAKE_HOST=t14g4 FAKE_SQUEUE_RUNNING="702|$HOME/github/proj" \
  "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: 走っている main のジョブ 702"
assert_file_missing "$TEST_ROOT/sbatch.args"
# r995 から投げ、走っているジョブは NFS の入口の下から投げられている
cd "$HOME/github/proj" || exit 1
out=$(SLURM_RUN_NFS_ENTRY=$NFS_ENTRY FAKE_SQUEUE_RUNNING="703|$NFS_ENTRY/proj" \
  "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: 走っている main のジョブ 703"

it "slurm-run: slurm-run.sh の外から投げた main のジョブも、作業ディレクトリで比べて --timed のジョブを止める"
cd "$HOME/github/committed" || exit 1
rm -f "$TEST_ROOT/sbatch.args"
out=$(FAKE_SQUEUE_RUNNING="704|$HOME/github/committed" "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: 走っている main のジョブ 704"
assert_contains "$(cat "$TEST_ROOT/squeue.args.running" 2>/dev/null)" "%i|%Z"

it "slurm-run: 走っている main のジョブを読めないときの警告に、squeue の理由を添える"
cd "$HOME/github/committed" || exit 1
out=$(FAKE_SQUEUE_RUNNING_EXIT=1 FAKE_SQUEUE_RUNNING_ERR="slurm_load_jobs error: Unable to contact slurm controller" \
  "$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' 2>&1)
assert_contains "$out" "WARN: 走っている main のジョブを読めず"
assert_contains "$out" "Unable to contact slurm controller"

it "slurm-run: 投げる前に止めたときと sbatch が投げられなかったときも、名前と成否の RESULT: を出す"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" "${PURPOSE[@]}" -t 25m xlc-early 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "RESULT: job=- name=xlc-early exit=1 state=NOT_SUBMITTED"
out=$("$RUN" "${PURPOSE[@]}" -t 5 xlc-early '/usr/bin/time xs' 2>&1)
assert_contains "$out" "RESULT: job=- name=xlc-early exit=1 state=NOT_SUBMITTED"
# 投げたジョブの RESULT: は 1 行だけ
out=$("$RUN" "${PURPOSE[@]}" -t 5 xlc-2G 'true' 2>&1)
assert_eq 1 "$(grep -c '^RESULT:' <<<"$out")"

it "slurm-run: /usr/bin/time の直後がリダイレクトや } でも止める"
for cmd in '/usr/bin/time>t.txt xs' '{ /usr/bin/time; }' '/usr/bin/time<in xs'; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${PURPOSE[@]}" -t 5 xlc-2G "$cmd" 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR: /usr/bin/time"
done

finish
