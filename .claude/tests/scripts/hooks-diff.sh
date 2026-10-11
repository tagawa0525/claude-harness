#!/usr/bin/env bash
# .claude/tests/hooks-diff.sh（claude-hooks の 2 つの版の判定を同じコマンドの集まりで比べる）のテスト
#
# 実行: bash .claude/tests/scripts/hooks-diff.sh（まとめて実行: bash .claude/tests/scripts.sh）
# nix build を避けるため、OLD_BIN / NEW_BIN に偽の claude-hooks を渡す。

source "$(dirname "$0")/../lib.sh"

DIFF="$CLAUDE_DIR/tests/hooks-diff.sh"

# 偽の claude-hooks: 引数なしでルールの一覧を出し、pre-tool-use では $1 の文字列を含むコマンドを
# block-no-verify として止める（$1 が空なら何も止めない）
make_fake_hooks() {
  local path="$1" deny_pattern="$2"
  cat >"$path" <<EOF
#!/usr/bin/env bash
if [[ \$# -eq 0 ]]; then
  echo "Usage: claude-hooks pre-tool-use [--rule <name>]..." >&2
  echo "rules: block-no-verify, guard-git-add" >&2
  exit 2
fi
shift
rules=()
while [[ \$# -gt 0 ]]; do rules+=("\$2"); shift 2; done
cmd=\$(jq -r .tool_input.command)
if [[ -n "$deny_pattern" && "\$cmd" == *"$deny_pattern"* ]] &&
  { [[ \${#rules[@]} -eq 0 ]] || [[ " \${rules[*]} " == *" block-no-verify "* ]]; }; then
  echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"x"}}'
fi
EOF
  chmod +x "$path"
}

make_fake_hooks "$TEST_ROOT/strict" "--no-verify"
make_fake_hooks "$TEST_ROOT/lax" ""

it "hooks-diff: 同じ判定の版どうしなら、緩くも厳しくもならない"
out=$(OLD_BIN="$TEST_ROOT/strict" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF")
assert_eq 0 $?
assert_contains "$out" "LOOSENED: 0"
assert_contains "$out" "TIGHTENED: 0"
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

it "hooks-diff: 呼べないバイナリなら失敗する"
out=$(OLD_BIN="$TEST_ROOT/missing" NEW_BIN="$TEST_ROOT/strict" bash "$DIFF" 2>&1)
assert_eq 1 $?

finish
