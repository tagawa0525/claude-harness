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
exit "${FAKE_JOB_EXIT:-0}"
EOF
cat >"$FAKE_BIN/scontrol" <<'EOF'
#!/usr/bin/env bash
echo "$*" >>"$TEST_ROOT/scontrol.calls"
EOF
cat >"$FAKE_BIN/hostname" <<'EOF'
#!/usr/bin/env bash
echo "${FAKE_HOST:-r995}"
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH" TEST_ROOT

RUN="$SCRIPTS_DIR/slurm-run.sh"
mkdir -p "$HOME/github/proj" "$HOME/r995/proj"

it "slurm-run: 排他・待機・環境の引き継ぎ・名前・ログを付けてコマンドを投げる"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" xlc-2G 'cargo run --release -- p2/assembly-2G.toml')
assert_eq 0 $?
args=$(cat "$TEST_ROOT/sbatch.args")
assert_contains "$args" "--exclusive"
assert_contains "$args" "--wait"
assert_contains "$args" "--export=ALL"
assert_contains "$args" $'-J\nxlc-2G'
assert_contains "$args" $'-o\n'"$HOME/github/slurm-logs/%x-%j.out"
assert_contains "$args" $'--wrap\ncargo run --release -- p2/assembly-2G.toml'

it "slurm-run: ジョブの番号、ログの場所、ログの末尾、終了コードを出す"
assert_contains "$out" "JOB: 42"
assert_contains "$out" "LOG: $HOME/github/slurm-logs/xlc-2G-42.out"
assert_contains "$out" "job output"
assert_contains "$out" "EXIT: 0"

it "slurm-run: ジョブが失敗したら、その終了コードで終わる"
out=$(FAKE_JOB_EXIT=3 "$RUN" xlc-2G 'false')
assert_eq 3 $?
assert_contains "$out" "EXIT: 3"

it "slurm-run: --top を付けると、投げたジョブを待ち行列の先頭に移す"
rm -f "$TEST_ROOT/scontrol.calls"
"$RUN" --top xlc-0005-1B 'true' >/dev/null
assert_eq "top 42" "$(cat "$TEST_ROOT/scontrol.calls")"

it "slurm-run: --top を付けなければ順番に触らない"
rm -f "$TEST_ROOT/scontrol.calls"
"$RUN" xlc-2G 'true' >/dev/null
assert_file_missing "$TEST_ROOT/scontrol.calls"

it "slurm-run: r995 以外では ~/r995 の下で投げ、ログは NFS の側に作る"
rm -rf "$HOME/github/slurm-logs" "$HOME/r995/slurm-logs"
cd "$HOME/r995/proj" || exit 1
out=$(FAKE_HOST=t14g4 "$RUN" xlc-2G 'true')
assert_eq 0 $?
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'-o\n'"$HOME/github/slurm-logs/%x-%j.out"
assert_contains "$out" "LOG: $HOME/r995/slurm-logs/xlc-2G-42.out"
assert_file_exists "$HOME/r995/slurm-logs"

it "slurm-run: r995 以外で ~/r995 の外から投げたら、投げずにエラー"
rm -f "$TEST_ROOT/sbatch.args"
cd "$HOME/github/proj" || exit 1
out=$(FAKE_HOST=t14g4 "$RUN" xlc-2G 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"
assert_file_missing "$TEST_ROOT/sbatch.args"

it "slurm-run: 名前がファイル名に使えない文字を含んだらエラー"
cd "$HOME/github/proj" || exit 1
out=$("$RUN" 'xlc 2G/a' 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR:"

it "slurm-run: 名前かコマンドがなければ使い方を出してエラー"
out=$("$RUN" xlc-2G 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage:"

finish
