#!/usr/bin/env bash
# .claude/scripts/slurm-run.sh（r995 の Slurm に重い計算を 1 本投げて終わりを待つ）のテスト
#
# 実行: bash .claude/tests/scripts/slurm-run.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

# sbatch、scontrol、hostname の偽物。sbatch は引数を記録し、-o の %x と %j を埋めたログに書き、
# FAKE_JOB_EXIT で終わる（sbatch --wait はジョブの終了コードで終わる）
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/sbatch" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"$TEST_ROOT/sbatch.args"
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
# show job は終わったジョブの RunTime と TimeLimit を返す（FAKE_SHOW_EXIT で失敗させられる）
if [[ $1 == show ]]; then
  echo "JobId=42 JobName=x RunTime=${FAKE_RUN_TIME:-00:12:34} TimeLimit=${FAKE_TIME_LIMIT:-00:30:00} TimeMin=N/A"
  # 本物は RunTime の行より後、Comment や WorkDir の行より前に、最後に止めたか再開した時刻を出す（一度も無ければ None）
  [[ -n ${FAKE_NO_SUSPEND_FIELD:-} ]] ||
    echo "   SuspendTime=${FAKE_SUSPEND_TIME:-None} SecsPreSuspend=0 LastSchedEval=2026-10-23T22:00:00"
  # 本物は SuspendTime の次の行に、ジョブが走ったパーティションを出す。既定は投げたときの --partition（無ければ main）
  part=main
  grep -qx -- '--partition=timed' "$TEST_ROOT/sbatch.args" && part=timed
  [[ -n ${FAKE_NO_PARTITION_FIELD:-} ]] || echo "   Partition=${FAKE_PARTITION:-$part} AllocNode:Sid=localhost:1"
  [[ -z ${FAKE_EXTRA_LINE:-} ]] || echo "$FAKE_EXTRA_LINE"
  exit "${FAKE_SHOW_EXIT:-0}"
fi
echo "$*" >>"$TEST_ROOT/scontrol.calls"
exit "${FAKE_TOP_EXIT:-0}"
EOF
# squeue は slurm-queue.sh が読む待ち行列。FAKE_SQUEUE_OUT を出して FAKE_SQUEUE_EXIT で終わる（既定は空）
cat >"$FAKE_BIN/squeue" <<'EOF'
#!/usr/bin/env bash
[[ -z ${FAKE_SQUEUE_OUT:-} ]] || printf '%s\n' "$FAKE_SQUEUE_OUT"
exit "${FAKE_SQUEUE_EXIT:-0}"
EOF
cat >"$FAKE_BIN/hostname" <<'EOF'
#!/usr/bin/env bash
echo "${FAKE_HOST:-r995}"
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH" TEST_ROOT
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

it "slurm-run: 時間を読めなくても、警告してジョブの終了コードで終わる"
out=$(FAKE_SHOW_EXIT=1 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'false' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_not_contains "$out" "TIME:"

it "slurm-run: ログに書き足せなくても、警告してジョブの終了コードで終わる"
out=$(FAKE_LOG_READONLY=1 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" -t 30 xlc-ro 'false' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_contains "$out" "EXIT: 3"

it "slurm-run: 実際の時間と見積もりは、ジョブの欄の先頭の RunTime と TimeLimit から読む"
# 本物の scontrol show job は複数行で、後ろの行に利用者が決める値（Comment、WorkDir など）が続く
out=$(FAKE_EXTRA_LINE="   Comment=feat/x RunTime=99:99:99 TimeLimit=9-99:99:99" "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"

it "slurm-run: -c <コア数> を付けると、--exclusive を付けず --cpus-per-task を渡し、連絡先に印を付ける"
cd "$HOME/github/proj" || exit 1
"$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G 'true' >/dev/null
args=$(cat "$TEST_ROOT/sbatch.args")
assert_not_contains "$args" "--exclusive"
assert_contains "$args" "--cpus-per-task=8"
assert_contains "$args" "--wait"
assert_contains "$args" "shared-cpus=8"

it "slurm-run: -c を付けたジョブの TIME 行とログの末尾に、時間は当てにならない印を付ける"
out=$("$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 shared (-c 8): 時間は当てにならない"
assert_contains "$(cat "$HOME/github/slurm-logs/openmc-2G-42.out")" "shared (-c 8): 時間は当てにならない"

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

it "slurm-run: --timed のパーティションは、Slurm の設定（modules/slurm/common.nix）の割り込まれないパーティションと同じ"
part=$(grep -o '"[a-z]* Nodes=[^"]*PreemptMode=OFF' "$CLAUDE_DIR/../modules/slurm/common.nix" | cut -d' ' -f1 | tr -d '"')
assert_ne "" "$part"
"$RUN" "${PURPOSE[@]}" --timed -t 5 xlc-bench 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "--partition=$part"

it "slurm-run: --timed は -c と組み合わせられず、投げずにエラー"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" "${PURPOSE[@]}" --timed -c 4 -t 5 xlc-bench 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: --timed を付けなければ、Slurm の設定の既定のパーティションを明示して投げる（SBATCH_PARTITION に左右されない）"
default_part=$(grep -o '"[a-z]* Nodes=[^"]*Default=YES' "$CLAUDE_DIR/../modules/slurm/common.nix" | cut -d' ' -f1 | tr -d '"')
assert_ne "" "$default_part"
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
out=$(FAKE_PARTITION=main "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
out=$(FAKE_PARTITION=timed "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_not_contains "$out" "preemptible"

it "slurm-run: パーティションは、利用者が決める値（Comment など）でなく、本物の Partition の欄から読む"
out=$(FAKE_EXTRA_LINE="   Comment=feat/x Partition=timed" "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$out" "preemptible:"

it "slurm-run: scontrol の出力に Partition の欄が無ければ、割り込まれうるかを確かめられないと警告し、印を付ける"
out=$(FAKE_NO_PARTITION_FIELD=1 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "Partition"
assert_contains "$out" "partition=unknown: 割り込まれうるかを確かめられないので時間は記録に使わない"

it "slurm-run: ジョブが止められていたら（SuspendTime がある）、TIME 行とログの末尾に、時間は当てにならない印を付ける"
out=$(FAKE_SUSPEND_TIME=2026-10-23T22:10:00 "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 suspended: 止められていたので時間は当てにならない"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-bench-42.out")" "suspended: 止められていたので時間は当てにならない"

it "slurm-run: 止められていない（SuspendTime=None）ジョブの TIME 行には、止められた印を付けない"
out=$("$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_not_contains "$out" "suspended"

it "slurm-run: 止められたかは、利用者が決める値（Comment など）でなく、本物の SuspendTime の欄から読む"
out=$(FAKE_EXTRA_LINE="   Comment=feat/x SuspendTime=2026-10-23T22:10:00" "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true')
assert_not_contains "$out" "suspended"

it "slurm-run: scontrol の出力に SuspendTime の欄が無ければ、止められたかを確かめられないと警告する"
out=$(FAKE_NO_SUSPEND_FIELD=1 FAKE_JOB_EXIT=3 "$RUN" "${PURPOSE[@]}" --timed -t 30 xlc-bench 'true' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "SuspendTime"
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00 suspended=unknown: 止められたかを確かめられないので時間は当てにならない"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-bench-42.out")" "suspended=unknown"

it "slurm-run: 割り込まれうるジョブが止められていたら、両方の印を付ける"
out=$(FAKE_SUSPEND_TIME=2026-10-23T22:10:00 "$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
assert_contains "$out" "suspended: 止められていたので時間は当てにならない"

it "slurm-run: 排他でなく、止められてもいたら、すべての印を付ける"
out=$(FAKE_SUSPEND_TIME=2026-10-23T22:10:00 "$RUN" "${PURPOSE[@]}" -c 8 -t 30 openmc-2G 'true')
assert_contains "$out" "shared (-c 8): 時間は当てにならない"
assert_contains "$out" "preemptible: 割り込まれうるので時間は記録に使わない"
assert_contains "$out" "suspended: 止められていたので時間は当てにならない"

it "slurm-run: -c が正の整数でなければ、投げずにエラー"
for bad in 0 -1 abc 1.5 ""; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" "${PURPOSE[@]}" -c "$bad" -t 30 openmc-2G 'true' 2>&1)
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

it "slurm-run: 並べ替えに失敗しても、警告してジョブの終わりまで待つ"
out=$(FAKE_TOP_EXIT=1 FAKE_JOB_EXIT=3 FAKE_SQUEUE_OUT="$(printf '%s\n' \
  "20|main|t|PENDING|10010|10:00|N/A|trial | #1 | p | $Q" \
  "42|main|new|PENDING|10000|10:00|N/A|trial | #1 | p | $Q")" \
  "$RUN" "${PURPOSE[@]}" --top -t 5 xlc-0005-1B 'true' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "EXIT: 3"

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

it "slurm-run: 既定の NFS の入口は、Nix のマウント点（modules/slurm/nfs-entry.nix）と同じ"
nix_entry=$(grep -o '^"[^"]*"$' "$CLAUDE_DIR/../modules/slurm/nfs-entry.nix" | tr -d '"')
assert_ne "" "$nix_entry"
assert_eq "$nix_entry" "$(grep -o 'SLURM_RUN_NFS_ENTRY:-[^}]*' "$RUN" | cut -d- -f2-)"

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

finish
