#!/usr/bin/env bash
# claude-hooks のバイナリを受け取るテスト（.claude/tests/hooks.sh、modules/home/parts/tests/main-commit-gates.sh）が、
# 走らせられないバイナリを渡されたら、各テストを失敗として数えずに、その場で止まることのテスト
#
# 実行: bash .claude/tests/scripts/hooks-bin.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

ROOT=$(cd "$CLAUDE_DIR/.." && pwd)
# 止まらなかったときに、main-commit-gates.sh が NixOS の構成を nix build しないよう、nix の無い PATH で走らせる
NO_NIX_PATH=$(minimal_path)
# 起動しても使い方を出さないバイナリ（壊れたバイナリの代わり）
make_fake_tool not-hooks "  *) exit 0 ;;"

# expect_stop <説明> <CLAUDE_HOOKS_BIN> <文言>: どちらのテストも、<文言> を出して終了コード 1 で止まる
expect_stop() {
  local what="$1" bin="$2" message="$3" target out
  for target in .claude/tests/hooks.sh modules/home/parts/tests/main-commit-gates.sh; do
    it "$target: $what CLAUDE_HOOKS_BIN ならその場で止まる"
    out=$(cd "$ROOT" && PATH="$NO_NIX_PATH" CLAUDE_HOOKS_BIN="$bin" bash "$target" 2>&1)
    assert_eq 1 $?
    assert_contains "$out" "$message"
    assert_not_contains "$out" "passed:"
  done
}

expect_stop "無いパスの" "$TEST_ROOT/missing/bin/claude-hooks" "claude-hooks を走らせられません"
expect_stop "ディレクトリの" "$TEST_ROOT" "claude-hooks を走らせられません"
expect_stop "使い方を出さないバイナリの" "$TEST_ROOT/fakebin/not-hooks" "claude-hooks を走らせられません"
# テストは一時のリポジトリへ cd してからバイナリを呼ぶので、相対のパスは解けなくなる
expect_stop "相対のパスの" "./result/bin/claude-hooks" "絶対パスで渡してください"
# nix build が失敗したときの `$(nix build … | tail -1)/bin/claude-hooks` の形
expect_stop "nix build の失敗の文言の" "error: Cannot build '/nix/store/x-claude-hooks-0.1.0.drv'./bin/claude-hooks" \
  "絶対パスで渡してください"

finish
