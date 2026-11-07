#!/usr/bin/env bash
# nix でビルドした claude-hooks の cap-review-rounds が、包んで渡す review-level.sh の ROUNDS で /code-review の周を
# 止めることを確かめる（package.nix の installCheckPhase が呼ぶ）。数え方の細部は、偽の review-level.sh を使う
# ユニットテスト（src/rules/cap_review_rounds.rs）が見る。
#
# 使い方: review-rounds.sh <claude-hooks の実行ファイル>
set -euo pipefail

BIN=$1
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
REPO="$ROOT/app"
git init -q -b main "$REPO"
git -C "$REPO" config core.hooksPath /nonexistent/hooks
echo init >"$REPO/README.md"
git -C "$REPO" add README.md
git -C "$REPO" commit -q -m init

review() {
  jq -n --arg a "high $REPO" '{tool_name: "Skill", tool_input: {skill: "code-review", args: $a}, cwd: "/"}' |
    "$BIN" pre-tool-use --rule cap-review-rounds
}

fail() {
  echo "FAIL: $1" >&2
  echo "$2" >&2
  exit 1
}

# 周を n 回通し、次の回が止まることを確かめる
expect_limit() {
  local limit=$1 out decision
  for ((i = 1; i <= limit; i++)); do
    out=$(review)
    decision=$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")
    [ -z "$decision" ] || fail "$i 周目は通すはず（上限 $limit）" "$out"
    jq -e '.systemMessage == null' <<<"$out" >/dev/null || fail "review-level.sh を読めるはず" "$out"
  done
  out=$(review)
  decision=$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")
  [ "$decision" = deny ] || fail "$((limit + 1)) 周目は止めるはず" "$out"
}

git -C "$REPO" switch -q -c docs/a
mkdir -p "$REPO/docs"
echo plan >"$REPO/docs/plan.md"
git -C "$REPO" add docs
git -C "$REPO" commit -q -m plan
expect_limit 2

git -C "$REPO" switch -q -c feat/b main
echo code >"$REPO/x.sh"
git -C "$REPO" add x.sh
git -C "$REPO" commit -q -m code
expect_limit 7

echo "ok: cap-review-rounds"
