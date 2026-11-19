#!/usr/bin/env bash
# claude/scripts/slurm-history.sh（jobcomp と sacct の記録から見積もりと実績を比べる）の配備の確認
#
# 実行: bash claude/tests/scripts/slurm-history.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script slurm-history（ADR-0101）。表の場合分けと版の決め方は
# crates/claude-hooks/src/script/slurm_history.rs、記録の読み方は slurm.rs の単体のテストが、sacct を呼ぶ形と git の
# reflog の読み方は crates/claude-hooks/tests/script_slurm_history.rs が確かめ、ここでは sh からブランチでビルドした
# バイナリを呼べることと、表、--job、読めない行の終了コードを確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

# sacct の偽物。--json の呼び出しには FAKE_SACCT_JOB（sacct --json=v0.0.45 の形）を、--json の無い呼び出しには
# State の文字の出力を出す
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/sacct" <<'EOF'
#!/usr/bin/env bash
if [[ " $* " == *" --json=v0.0.45 "* ]]; then
  printf '%s\n' "$FAKE_SACCT_JOB"
else
  echo COMPLETED
fi
EOF
chmod +x "$FAKE_BIN"/*
export PATH="$FAKE_BIN:$PATH"

HISTORY="$SCRIPTS_DIR/slurm-history.sh"
LOG="$TEST_ROOT/job_completions"

# job <id> <name> <limit> <start> <end>: jobcomp/filetxt の 1 行（COMPLETED、main）
job() {
  printf 'JobId=%s Name=%s JobState=COMPLETED Partition=main TimeLimit=%s StartTime=%s EndTime=%s NodeCnt=1 ProcCnt=32 \n' "$@"
}

{
  job 73 openmc-2A 45 2026-10-11T13:45:32 2026-10-11T14:24:05
  job 74 openmc-2A 45 2026-10-11T14:24:05 2026-10-11T15:14:32
} >"$LOG"

it "slurm-history: sh から、ジョブごとの見積もり・実績・比率の表を出す"
out=$("$HISTORY" "$LOG")
assert_eq 0 $?
assert_contains "$out" "     73  openmc-2A                   0:45:00    0:38:33    0.86        -"
assert_contains "$out" "SKIPPED: 0"

it "slurm-history: sh から、--by-name でジョブ名ごとに集計する"
out=$("$HISTORY" --by-name "$LOG")
assert_eq 0 $?
assert_contains "$out" "openmc-2A                    2    0:45:00    0:44:30    0.99      1.12        -"

it "slurm-history: 読めない行は、行番号を出して exit 1"
{
  job 1 ok 10 2026-10-11T10:00:00 2026-10-11T10:05:00
  echo "JobId=2 Name=broken JobState=COMPLETED TimeLimit=10 NodeCnt=1"
} >"$LOG"
out=$("$HISTORY" "$LOG" 2>&1)
assert_eq 1 $?
assert_contains "$out" "ERROR: 2 行目を読めません（StartTime がありません）"

it "slurm-history: sh から、--job で sacct のジョブの行を出す"
export FAKE_SACCT_JOB='{"jobs":[{"job_id":101,"name":"job-101","time":{"elapsed":600,"submission":1795077405,"start":1795077405,"end":1795078005},"working_directory":"/nonexistent","submit_line":"/x/sbatch --exclusive --wrap lattice input.toml"}]}'
out=$("$HISTORY" --job 101)
assert_eq 0 $?
assert_contains "$out" "JOB: 101 job-101"
assert_contains "$out" "STATE: COMPLETED"
assert_contains "$out" "EXCLUSIVE: yes"
assert_contains "$out" "REASON: commit= もブランチも無い"
assert_contains "$out" "COMMAND: lattice input.toml"

finish
