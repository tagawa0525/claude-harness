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
  [[ -z ${FAKE_EXTRA_LINE:-} ]] || echo "$FAKE_EXTRA_LINE"
  exit "${FAKE_SHOW_EXIT:-0}"
fi
echo "$*" >>"$TEST_ROOT/scontrol.calls"
exit "${FAKE_TOP_EXIT:-0}"
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
mkdir -p "$HOME/github/proj" "$NFS_ENTRY/proj"

it "slurm-run: 排他・待機・環境の引き継ぎ・名前・ログ・見積もり・連絡先を付けてコマンドを投げる"
cd "$HOME/github/proj" || exit 1
git init -q -b feat/vera-2g .
out=$("$RUN" -t 30 xlc-2G 'cargo run --release -- p2/assembly-2G.toml')
assert_eq 0 $?
args=$(cat "$TEST_ROOT/sbatch.args")
assert_contains "$args" "--exclusive"
assert_contains "$args" "--wait"
assert_contains "$args" "--export=ALL"
assert_contains "$args" $'-J\nxlc-2G'
assert_contains "$args" $'-o\n'"$HOME/github/slurm-logs/%x-%j.out"
assert_contains "$args" $'--wrap\ncargo run --release -- p2/assembly-2G.toml'
assert_contains "$args" $'--time\n30'
assert_contains "$args" $'--comment\nfeat/vera-2g '"$HOME/github/proj"

it "slurm-run: ジョブの番号、ログの場所、ログの末尾、終了コードを出す"
assert_contains "$out" "JOB: 42"
assert_contains "$out" "LOG: $HOME/github/slurm-logs/xlc-2G-42.out"
assert_contains "$out" "job output"
assert_contains "$out" "EXIT: 0"

it "slurm-run: 終わったジョブの実際の時間と見積もりを出し、ログの末尾にも書く"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" -t 30 xlc-2G 'true')
assert_contains "$out" "TIME: elapsed=00:12:34 estimate=00:30:00"
assert_contains "$(cat "$HOME/github/slurm-logs/xlc-2G-42.out")" "TIME: elapsed=00:12:34 estimate=00:30:00"

it "slurm-run: ジョブが失敗しても、実際の時間と見積もりを出す"
out=$(FAKE_JOB_EXIT=3 "$RUN" -t 30 xlc-2G 'false')
assert_eq 3 $?
assert_contains "$out" "TIME: elapsed=00:12:34 estimate=00:30:00"

it "slurm-run: 時間を読めなくても、警告してジョブの終了コードで終わる"
out=$(FAKE_SHOW_EXIT=1 FAKE_JOB_EXIT=3 "$RUN" -t 30 xlc-2G 'false' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_not_contains "$out" "TIME:"

it "slurm-run: ログに書き足せなくても、警告してジョブの終了コードで終わる"
out=$(FAKE_LOG_READONLY=1 FAKE_JOB_EXIT=3 "$RUN" -t 30 xlc-ro 'false' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "TIME: elapsed=00:12:34 estimate=00:30:00"
assert_contains "$out" "EXIT: 3"

it "slurm-run: 実際の時間と見積もりは、ジョブの欄の先頭の RunTime と TimeLimit から読む"
# 本物の scontrol show job は複数行で、後ろの行に利用者が決める値（Comment、WorkDir など）が続く
out=$(FAKE_EXTRA_LINE="   Comment=feat/x RunTime=99:99:99 TimeLimit=9-99:99:99" "$RUN" -t 30 xlc-2G 'true')
assert_contains "$out" "TIME: elapsed=00:12:34 estimate=00:30:00"

it "slurm-run: -c <コア数> を付けると、--exclusive を付けず --cpus-per-task を渡し、連絡先に印を付ける"
cd "$HOME/github/proj" || exit 1
"$RUN" -c 8 -t 30 openmc-2G 'true' >/dev/null
args=$(cat "$TEST_ROOT/sbatch.args")
assert_not_contains "$args" "--exclusive"
assert_contains "$args" "--cpus-per-task=8"
assert_contains "$args" "--wait"
assert_contains "$args" "shared-cpus=8"

it "slurm-run: -c を付けたジョブの TIME 行とログの末尾に、時間は当てにならない印を付ける"
out=$("$RUN" -c 8 -t 30 openmc-2G 'true')
assert_contains "$out" "TIME: elapsed=00:12:34 estimate=00:30:00 shared (-c 8): 時間は当てにならない"
assert_contains "$(cat "$HOME/github/slurm-logs/openmc-2G-42.out")" "shared (-c 8): 時間は当てにならない"

it "slurm-run: -c を付けなければ、排他で投げ、TIME 行に印を付けない"
out=$("$RUN" -t 30 xlc-2G 'true')
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" "--exclusive"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "--cpus-per-task"
assert_not_contains "$(cat "$TEST_ROOT/sbatch.args")" "shared-cpus"
assert_not_contains "$out" "shared"

it "slurm-run: -c が正の整数でなければ、投げずにエラー"
for bad in 0 -1 abc 1.5 ""; do
  rm -f "$TEST_ROOT/sbatch.args"
  out=$("$RUN" -c "$bad" -t 30 openmc-2G 'true' 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "ERROR:"
  assert_file_missing "$TEST_ROOT/sbatch.args"
done

it "slurm-run: -c にコア数がなければ使い方を出してエラー"
out=$("$RUN" -t 30 -c 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

it "slurm-run: ジョブが失敗したら、その終了コードで終わる"
out=$(FAKE_JOB_EXIT=3 "$RUN" -t 1 xlc-2G 'false')
assert_eq 3 $?
assert_contains "$out" "EXIT: 3"

it "slurm-run: --top を付けると、投げたジョブを待ち行列の先頭に移す"
rm -f "$TEST_ROOT/scontrol.calls"
"$RUN" --top -t 5 xlc-0005-1B 'true' >/dev/null
assert_eq "top 42" "$(cat "$TEST_ROOT/scontrol.calls")"

it "slurm-run: 並べ替えに失敗しても、警告してジョブの終わりまで待つ"
out=$(FAKE_TOP_EXIT=1 FAKE_JOB_EXIT=3 "$RUN" --top -t 5 xlc-0005-1B 'true' 2>&1)
assert_eq 3 $?
assert_contains "$out" "WARN:"
assert_contains "$out" "EXIT: 3"

it "slurm-run: --top を付けなければ順番に触らない"
rm -f "$TEST_ROOT/scontrol.calls"
"$RUN" -t 1 xlc-2G 'true' >/dev/null
assert_file_missing "$TEST_ROOT/scontrol.calls"

it "slurm-run: git の外では連絡先に場所だけを書く"
mkdir -p "$HOME/github/plain" && cd "$HOME/github/plain" || exit 1
"$RUN" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\n'"$HOME/github/plain"

it "slurm-run: Claude Code から投げたら、連絡先にセッションの ID を足す"
cd "$HOME/github/proj" || exit 1
CLAUDE_CODE_SESSION_ID=abc-123 "$RUN" -t 1 xlc-2G 'true' >/dev/null
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\nfeat/vera-2g '"$HOME/github/proj claude=abc-123"

it "slurm-run: r995 以外では NFS の入口の下で投げ、ログは NFS の側に作る"
rm -rf "$HOME/github/slurm-logs" "$NFS_ENTRY/slurm-logs"
cd "$NFS_ENTRY/proj" || exit 1
out=$(SLURM_RUN_NFS_ENTRY=$NFS_ENTRY FAKE_HOST=t14g4 "$RUN" -t 1 xlc-2G 'true')
assert_eq 0 $?
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'-o\n'"$HOME/github/slurm-logs/%x-%j.out"
assert_contains "$out" "LOG: $NFS_ENTRY/slurm-logs/xlc-2G-42.out"
assert_file_exists "$NFS_ENTRY/slurm-logs"

it "slurm-run: r995 以外で NFS の入口の外から投げたら、投げずに既定の入口 /mnt/r995/github を示してエラー"
rm -f "$TEST_ROOT/sbatch.args"
cd "$HOME/github/proj" || exit 1
out=$(FAKE_HOST=t14g4 "$RUN" -t 1 xlc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_contains "$out" "/mnt/r995/github"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: 既定の NFS の入口は、Nix のマウント点（modules/slurm/nfs-entry.nix）と同じ"
nix_entry=$(grep -o '^"[^"]*"$' "$CLAUDE_DIR/../modules/slurm/nfs-entry.nix" | tr -d '"')
[[ -n $nix_entry ]]
assert_eq 0 $?
assert_eq "$nix_entry" "$(grep -o 'SLURM_RUN_NFS_ENTRY:-[^}]*' "$RUN" | cut -d- -f2-)"

it "slurm-run: 名前がファイル名に使えない文字を含んだらエラー"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" -t 1 'xlc 2G/a' 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "slurm-run: 名前かコマンドがなければ使い方を出してエラー"
out=$("$RUN" -t 1 xlc-2G 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

it "slurm-run: 見積もりの時間がなければ使い方を出してエラー"
out=$("$RUN" xlc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

finish
