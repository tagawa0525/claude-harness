#!/usr/bin/env bash
# claude-hooks のバイナリを受け取るテスト（.claude/tests/hooks.sh、modules/home/parts/tests/main-commit-gates.sh）が、
# 走らせられないバイナリを渡されたら、各テストを失敗として数えずに、その場で止まることのテスト
#
# 実行: bash .claude/tests/scripts/hooks-bin.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

ROOT=$(cd "$CLAUDE_DIR/.." && pwd)
# nix build が失敗したときの `$(nix build … | tail -1)/bin/claude-hooks` の形
BROKEN="error: Cannot build '/nix/store/x-claude-hooks-0.1.0.drv'./bin/claude-hooks"

for target in .claude/tests/hooks.sh modules/home/parts/tests/main-commit-gates.sh; do
  it "$target: 走らせられない CLAUDE_HOOKS_BIN ならその場で止まる"
  out=$(cd "$ROOT" && CLAUDE_HOOKS_BIN="$BROKEN" bash "$target" 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "claude-hooks を走らせられません"
  assert_not_contains "$out" "passed:"
done

finish
