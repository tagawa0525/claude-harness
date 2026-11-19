#!/usr/bin/env bash
# claude/scripts/slurm-run.sh（r995 の Slurm に重い計算を 1 本投げて終わりを待つ）の配備の確認
#
# 実行: bash claude/tests/scripts/slurm-run.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script slurm-run。引数の確かめ方、TIME: の印、Slurm の JSON の読み方、並べ直しと待ちの回数は
# crates/claude-hooks/src/script/slurm_run.rs と slurm.rs の単体のテストが、投げる、並べ直す、待つ、中断の流れは
# crates/claude-hooks/tests/script_slurm_run.rs が確かめ、ここでは sh からブランチでビルドしたバイナリを呼べることと、
# 投げたジョブの出力の行、投げる前に止めたときの終了コードを確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

# sbatch、squeue、sacct、hostname の偽物。sbatch は番号を出してログに書き、sacct は終わったジョブの記録を JSON で返す
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
EOF
cat >"$FAKE_BIN/squeue" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$FAKE_BIN/sacct" <<'EOF'
#!/usr/bin/env bash
echo '{"jobs":[{"job_id":42,"partition":"main","state":{"current":["COMPLETED"]},"time":{"elapsed":754,"limit":{"set":true,"infinite":false,"number":30},"suspended":0}}]}'
EOF
cat >"$FAKE_BIN/hostname" <<'EOF'
#!/usr/bin/env bash
echo r995
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH" TEST_ROOT
export SLURM_RUN_SACCT_INTERVAL=0
unset CLAUDE_CODE_SESSION_ID SLURM_RUN_NFS_ENTRY

RUN="$SCRIPTS_DIR/slurm-run.sh"
PURPOSE=(--class trial --ref '#1' --purpose '試験の目的')
mkdir -p "$HOME/github/proj"
cd "$HOME/github/proj" || exit 1

it "slurm-run: sh から投げ、JOB:、LOG:、ログの末尾、TIME 行、RESULT:、EXIT: を出して、ジョブの終了コードで終わる"
out=$("$RUN" "${PURPOSE[@]}" -t 30 xlc-2G 'true')
assert_eq 0 $?
assert_contains "$out" "JOB: 42"
assert_contains "$out" "LOG: $HOME/github/slurm-logs/xlc-2G-42.out"
assert_contains "$out" "job output"
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_contains "$out" "RESULT: job=42 name=xlc-2G exit=0 state=COMPLETED"
assert_contains "$out" "EXIT: 0"
assert_contains "$(cat "$TEST_ROOT/sbatch.args")" $'--comment\ntrial | #1 | 試験の目的 | '"$HOME/github/proj"

it "slurm-run: sh から、投げる前に止めたら、理由と RESULT: を出して 1 で終わる"
rm -f "$TEST_ROOT/sbatch.args"
out=$("$RUN" "${PURPOSE[@]}" -t 25m xlc-early 'true' 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: -t は分の数"
assert_contains "$out" "RESULT: job=- name=xlc-early exit=1 state=NOT_SUBMITTED"
assert_file_missing "$TEST_ROOT/sbatch.args"

finish
