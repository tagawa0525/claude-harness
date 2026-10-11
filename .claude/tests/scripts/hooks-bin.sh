#!/usr/bin/env bash
# claude-hooks のバイナリを受け取るテスト（.claude/tests/hooks.sh、modules/home/parts/tests/main-commit-gates.sh）が、
# 走らせられないバイナリを渡されたら、各テストを失敗として数えずに、その場で止まることのテスト
#
# 実行: bash .claude/tests/scripts/hooks-bin.sh（まとめて実行: bash .claude/tests/scripts.sh）

source "$(dirname "$0")/../lib.sh"

ROOT=$(cd "$CLAUDE_DIR/.." && pwd)
# 止まらなかったときに、main-commit-gates.sh が NixOS の構成を nix build しないよう、nix の無い PATH で走らせる
NO_NIX_PATH=$(minimal_path)
# 起動しても使い方を出さないバイナリ（壊れたバイナリの代わり）。理由を標準エラーに出す
make_fake_tool not-hooks "  *) echo 'cannot load libfoo.so' >&2; exit 127 ;;"

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
expect_stop "使い方を出さないバイナリの、その理由を出して" "$TEST_ROOT/fakebin/not-hooks" "cannot load libfoo.so"
# テストは一時のリポジトリへ cd してからバイナリを呼ぶので、相対のパスは解けなくなる
expect_stop "相対のパスの" "./result/bin/claude-hooks" "絶対パスで渡してください"
# nix build が失敗したときの `$(nix build … | tail -1)/bin/claude-hooks` の形
expect_stop "nix build の失敗の文言の" "error: Cannot build '/nix/store/x-claude-hooks-0.1.0.drv'./bin/claude-hooks" \
  "絶対パスで渡してください"

# 引数なしで使い方を出して 2 で終わる（本物と同じ）。ただし標準入力が閉じていなければ使い方を出さない
# （標準入力を読むバイナリを、呼んだ側の端末につないで待たせない）
make_fake_tool hooks-like "  '') [[ \$(readlink /proc/self/fd/0) == /dev/null ]] || exit 1
    echo 'rules: block-main-commit' >&2; exit 2 ;;
  *) exit 0 ;;"
for target in .claude/tests/hooks.sh modules/home/parts/tests/main-commit-gates.sh; do
  it "$target: 使い方を出すバイナリは、標準入力を閉じて呼び、通す"
  out=$(cd "$ROOT" && PATH="$NO_NIX_PATH" CLAUDE_HOOKS_BIN="$TEST_ROOT/fakebin/hooks-like" bash "$target" 2>&1 </dev/zero)
  assert_not_contains "$out" "claude-hooks を走らせられません"
  assert_not_contains "$out" "絶対パス"
done

# 起動の確認は通り、ルールの呼び出しで失敗する版（ルールの足りない古い版など）
make_fake_tool rule-fails "  '') echo 'rules: block-main-commit' >&2; exit 2 ;;
  *) echo 'unknown rule: guard-sbatch' >&2; exit 2 ;;"
# ルールが panic する版（claude-hooks は panic を拾い、標準エラーに出して 0 で終わる）
make_fake_tool rule-panics "  '') echo 'rules: block-main-commit' >&2; exit 2 ;;
  *) echo \"thread 'main' panicked at src/shell.rs\" >&2; exit 0 ;;"
for bin in rule-fails rule-panics; do
  it ".claude/tests/hooks.sh: ルールの呼び出しで $bin なら、テストを失敗と数えずにその場で止まる"
  out=$(cd "$ROOT" && PATH="$NO_NIX_PATH" CLAUDE_HOOKS_BIN="$TEST_ROOT/fakebin/$bin" bash .claude/tests/hooks.sh 2>&1)
  assert_eq 1 $?
  assert_contains "$out" "claude-hooks が失敗しました"
  assert_not_contains "$out" "passed:"
done
it ".claude/tests/hooks.sh: ルールの呼び出しの失敗では、バイナリの標準エラーを出す"
out=$(cd "$ROOT" && PATH="$NO_NIX_PATH" CLAUDE_HOOKS_BIN="$TEST_ROOT/fakebin/rule-panics" bash .claude/tests/hooks.sh 2>&1)
assert_contains "$out" "thread 'main' panicked"

finish
