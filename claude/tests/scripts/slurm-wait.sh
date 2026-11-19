#!/usr/bin/env bash
# claude/scripts/slurm-wait.sh（既にある Slurm のジョブの終わりを番号から待つ）の配備の確認
#
# 実行: bash claude/tests/scripts/slurm-wait.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script slurm-wait。待ち方、終了コード、TIME: の行を重ねないことは
# crates/claude-hooks/tests/script_slurm_wait.rs が確かめ、ここでは sh からブランチでビルドしたバイナリを呼べることと、
# 終わりの行の形を確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

# scontrol、sacct、hostname の偽物。scontrol は終わったジョブを、sacct はその記録を JSON で返す
FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN" "$HOME/github/slurm-logs"
cat >"$FAKE_BIN/scontrol" <<'EOF'
#!/usr/bin/env bash
printf '{"jobs":[{"job_id":42,"name":"xlc-2G","comment":"trial | #1 | 試験の目的 | c","stdout_expanded":"%s/github/slurm-logs/xlc-2G-42.out","job_state":["COMPLETED"],"exit_code":{"return_code":{"set":true,"infinite":false,"number":0},"signal":{"id":{"set":false,"infinite":false,"number":0}}}}]}\n' "$HOME"
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
export PATH="$FAKE_BIN:$PATH"
export SLURM_WAIT_INTERVAL=0 SLURM_RUN_SACCT_INTERVAL=0
unset SLURM_RUN_NFS_ENTRY
echo "job output" >"$HOME/github/slurm-logs/xlc-2G-42.out"

WAIT="$SCRIPTS_DIR/slurm-wait.sh"

it "slurm-wait: sh から番号のジョブを待ち、JOB:、LOG:、ログの末尾、TIME 行、RESULT:、EXIT: を出して、ジョブの終了コードで終わる"
out=$("$WAIT" 42)
assert_eq 0 $?
assert_contains "$out" "JOB: 42"
assert_contains "$out" "LOG: $HOME/github/slurm-logs/xlc-2G-42.out"
assert_contains "$out" "job output"
assert_contains "$out" "TIME: job=42 elapsed=00:12:34 estimate=00:30:00"
assert_contains "$out" "class=trial ref=#1 purpose=試験の目的"
assert_contains "$out" "RESULT: job=42 name=xlc-2G exit=0 state=COMPLETED"
assert_contains "$out" "EXIT: 0"

it "slurm-wait: sh から、番号が無ければ使い方を出して 1 で終わる"
out=$("$WAIT" 2>&1)
assert_eq 1 $?
assert_contains "$out" "Usage: slurm-wait.sh"

finish
