#!/usr/bin/env bash
# .claude/tests/hooks-diff.sh（claude-hooks の 2 つの版の判定を同じコマンドの集まりで比べる）のテスト
#
# 実行: bash .claude/tests/scripts/hooks-diff.sh（まとめて実行: bash .claude/tests/scripts.sh）
# nix build を避けるため、OLD_BIN / NEW_BIN に偽の claude-hooks を渡す。

source "$(dirname "$0")/../lib.sh"

DIFF="$CLAUDE_DIR/tests/hooks-diff.sh"
# 組み合わせの集まりを全て回すと遅いので、確かめに要るコマンドだけを比べる
export HOOKS_DIFF_COMMANDS=$'git commit --no-verify -m x\ngit add -A\ngit status'

# make_fake_hooks <path> <rules> <spec>...: 偽の claude-hooks。引数なしで <rules>（", " 区切り）を一覧に出す。
# pre-tool-use --rule <r>... では、<spec>（<ルール>:<判定>:<文字列>）のうち、渡したルールのどれかに一致し、
# コマンドが文字列を含む最初のものの判定（deny / context）を出す（本物と同じく、複数のルールの判定を
# 1 つにまとめる）。<判定> が fail なら終了コード 3 で落ち、panic なら標準エラーに書いて終了コード 0 で終わる
# （本物は、ルールの panic を catch_unwind で拾い、何も出さずに 0 で終わる）
make_fake_hooks() {
  local path="$1" rules="$2"
  shift 2
  {
    echo '#!/usr/bin/env bash'
    echo "if [[ \$# -eq 0 ]]; then echo 'rules: $rules' >&2; exit 2; fi"
    # 入力の JSON の文字列のまま照らす（jq を呼ぶと、コマンドの数だけ遅くなる）
    echo 'args=" $* "; cmd=$(cat)'
    for spec in "$@"; do
      IFS=: read -r r d p <<<"$spec"
      echo "if [[ \$args == *' --rule $r '* && \$cmd == *'$p'* ]]; then"
      case "$d" in
        deny) echo "  echo '{\"hookSpecificOutput\":{\"permissionDecision\":\"deny\"}}'; exit 0" ;;
        context) echo "  echo '{\"hookSpecificOutput\":{\"additionalContext\":\"x\"}}'; exit 0" ;;
        fail) echo "  exit 3" ;;
        panic) echo "  echo 'thread main panicked' >&2; exit 0" ;;
      esac
      echo "fi"
    done
  } >"$path"
  chmod +x "$path"
}

make_fake_hooks "$TEST_ROOT/strict" "block-no-verify, guard-git-add" "block-no-verify:deny:--no-verify"
make_fake_hooks "$TEST_ROOT/lax" "block-no-verify, guard-git-add"

it "hooks-diff: HOOKS_DIFF_COMMANDS のコマンドだけを比べる"
out=$(OLD_BIN="$TEST_ROOT/lax" NEW_BIN="$TEST_ROOT/lax" bash "$DIFF")
assert_contains "$out" "CHECKED: 3"

it "hooks-diff: 同じ判定の版どうしなら、緩くも厳しくもならない"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF")
assert_eq 0 $?
assert_contains "$out" "LOOSENED: 0"
assert_contains "$out" "TIGHTENED: 0"
assert_contains "$out" "CHANGED: 0"
assert_contains "$out" "CHECKED: "

it "hooks-diff: 古い版が止め、新しい版が通すものを、ルールとコマンドの組で LOOSENED に出す"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/lax" bash "$DIFF")
assert_eq 0 $?
assert_contains "$out" "$(printf 'LOOSENED\tblock-no-verify\tgit commit --no-verify -m x')"
assert_not_contains "$out" "LOOSENED: 0"
assert_not_contains "$out" "$(printf 'LOOSENED\tguard-git-add\t')"
assert_contains "$out" "TIGHTENED: 0"

it "hooks-diff: 古い版が通し、新しい版が止めるものを TIGHTENED に出す"
out=$(OLD_BIN="$TEST_ROOT/lax" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF")
assert_eq 0 $?
assert_contains "$out" "$(printf 'TIGHTENED\tblock-no-verify\tgit commit --no-verify -m x')"
assert_contains "$out" "LOOSENED: 0"

it "hooks-diff: 別のルールが同じコマンドを止めていても、緩くなったルールを出す"
make_fake_hooks "$TEST_ROOT/both" "block-no-verify, guard-git-add" "guard-git-add:deny:git" "block-no-verify:deny:--no-verify"
make_fake_hooks "$TEST_ROOT/add-only" "block-no-verify, guard-git-add" "guard-git-add:deny:git"
out=$(OLD_BIN="$TEST_ROOT/both" NEW_BIN="$TEST_ROOT/add-only" bash "$DIFF")
assert_contains "$out" "$(printf 'LOOSENED\tblock-no-verify\tgit commit --no-verify -m x')"

it "hooks-diff: 新しい版で無くなったルールを LOOSENED に出す"
make_fake_hooks "$TEST_ROOT/one-rule" "block-no-verify" "block-no-verify:deny:--no-verify"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/one-rule" bash "$DIFF")
assert_contains "$out" "$(printf 'LOOSENED\tguard-git-add\t(新しい版に無い)')"
assert_contains "$out" "LOOSENED: 1"

it "hooks-diff: deny 以外の判定の変化（additionalContext など）を CHANGED に出す"
make_fake_hooks "$TEST_ROOT/context" "block-no-verify, guard-git-add" "block-no-verify:deny:--no-verify" "guard-git-add:context:git add -A"
out=$(OLD_BIN="$TEST_ROOT/context" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF")
assert_contains "$out" "$(printf 'CHANGED\tguard-git-add\tcontext -> allow\tgit add -A')"
assert_not_contains "$out" "CHANGED: 0"

it "hooks-diff: バイナリが失敗したら、通したものと数えずに失敗する"
make_fake_hooks "$TEST_ROOT/crash" "block-no-verify, guard-git-add" "block-no-verify:fail:git"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/crash" bash "$DIFF" 2>&1)
assert_eq 1 $?
assert_not_contains "$out" "LOOSENED"

it "hooks-diff: ルールが panic したら（標準エラーに出て終了コード 0）、通したものと数えずに失敗する"
make_fake_hooks "$TEST_ROOT/panic" "block-no-verify, guard-git-add" "block-no-verify:panic:git"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/panic" bash "$DIFF" 2>&1)
assert_eq 1 $?
assert_not_contains "$out" "LOOSENED"

it "hooks-diff: 古い版が一度も通す以外を返さなかったルールを UNCOVERED に出す"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF")
assert_contains "$out" "UNCOVERED: guard-git-add"
assert_not_contains "$out" "UNCOVERED: block-no-verify"

it "hooks-diff: 呼べないバイナリなら失敗する"
out=$(OLD_BIN="$TEST_ROOT/missing" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF" 2>&1)
assert_eq 1 $?

finish
