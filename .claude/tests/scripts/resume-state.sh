#!/usr/bin/env bash
# .claude/skills/resume/scripts/resume-state.sh（再開時の状態の収集）のテスト
#
# 実行: bash .claude/tests/scripts/resume-state.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

SCRIPT="$CLAUDE_DIR/skills/resume/scripts/resume-state.sh"

REPO="$TEST_ROOT/resume/app"
make_repo "$REPO"
cd "$REPO" || exit 1
mkdir -p docs/issues
printf -- '---\nstatus: open\nblocked-by: ["#0001"]\n---\n\n# a\n' > docs/issues/0002-a.md
printf -- '---\nstatus: closed\nresolution: done\n---\n\n# b\n' > docs/issues/0001-b.md
git add docs && git commit -q -m "docs: seed" -m "Claude-Session: sess-1"
git worktree add -q -b feat/x "$TEST_ROOT/resume/wt-x"
echo dirty > "$TEST_ROOT/resume/wt-x/new.txt"

it "resume-state: worktree のパス、ブランチ、未コミットの変更、Claude-Session を出す"
out=$("$SCRIPT")
assert_eq 0 $?
assert_contains "$out" "== worktrees"
assert_contains "$out" "$TEST_ROOT/resume/wt-x feat/x dirty session=sess-1"
assert_contains "$out" "$REPO main session=sess-1"

it "resume-state: docs/issues の開いている issue だけを blocked-by つきで出す"
assert_contains "$out" "docs/issues/0002-a.md blocked-by=[\"#0001\"]"
assert_not_contains "$out" "0001-b.md"

it "resume-state: origin が無ければ PR の節は理由を出して止まらない"
assert_contains "$out" "== prs"
assert_contains "$out" "(なし: gh か origin が無い)"
assert_contains "$out" "== slurm"

finish
