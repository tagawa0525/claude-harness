#!/usr/bin/env bash
# claude/skills/resume/scripts/resume-state.sh（再開時の状態の収集）の配備の確認
#
# 実行: bash claude/tests/scripts/resume-state.sh（まとめて実行: bash claude/tests/scripts.sh）

# 中身は claude-hooks script resume-state。節ごとの場合分けは crates/claude-hooks/tests/script_resume_state.rs が確かめ、
# ここではスキルの下の sh からブランチでビルドしたバイナリを呼べることと、節の見出しを確かめる
source "$(dirname "$0")/../claude-hooks-bin.sh"
source "$(dirname "$0")/../lib.sh"

SCRIPT="$CLAUDE_DIR/skills/resume/scripts/resume-state.sh"

REPO="$TEST_ROOT/resume/app"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p docs/issues
printf -- '---\nstatus: open\nblocked-by: ["#0001"]\n---\n\n# a\n' > docs/issues/0002-a.md
git add docs && git commit -q -m "docs: seed" -m "Claude-Session: sess-1"

it "resume-state: スキルの下の sh から、worktree、PR、Slurm、issue の節を出す"
out=$("$SCRIPT")
assert_eq 0 $?
assert_contains "$out" "== worktrees"
assert_contains "$out" "$REPO main session=sess-1"
assert_contains "$out" "== prs"
assert_contains "$out" "== slurm"
assert_contains "$out" "== issues"
assert_contains "$out" "docs/issues/0002-a.md blocked-by=[\"#0001\"]"

finish
