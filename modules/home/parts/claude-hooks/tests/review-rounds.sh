#!/usr/bin/env bash
# nix でビルドした claude-hooks の cap-review-rounds が、/code-review の周を止めずに数え、何周目かを伝えることを
# 確かめる（package.nix の installCheckPhase が呼ぶ）。数え方の細部はユニットテスト（src/rules/cap_review_rounds.rs）が見る。
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

# 周を n 回回し、どの回も止めずに周の数を伝えることを確かめる
expect_counted() {
  local rounds=$1 out decision
  for ((i = 1; i <= rounds; i++)); do
    out=$(review)
    decision=$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")
    [ -z "$decision" ] || fail "$i 周目も止めないはず" "$out"
    jq -e --arg n "$i 周目" '.hookSpecificOutput.additionalContext | contains($n)' <<<"$out" >/dev/null ||
      fail "$i 周目と伝えるはず" "$out"
    jq -e '.systemMessage == null' <<<"$out" >/dev/null || fail "判定できない理由は無いはず" "$out"
  done
}

git -C "$REPO" switch -q -c docs/a
mkdir -p "$REPO/docs"
echo plan >"$REPO/docs/plan.md"
git -C "$REPO" add docs
git -C "$REPO" commit -q -m plan
expect_counted 9

echo "ok: cap-review-rounds"
