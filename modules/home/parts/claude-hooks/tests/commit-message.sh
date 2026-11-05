#!/usr/bin/env bash
# nix でビルドした claude-hooks の check-commit-message が、包んで渡す commit-msg-check（commit-msg の hook と同じ
# 検査）で、git commit の件名が 72 文字を超えるメッセージと Conventional Commits でないメッセージを止め、通る件名と、
# 形式と長さを問わない Merge / fixup! / squash! / Revert の件名を通すことを確かめる（package.nix の installCheckPhase が呼ぶ）。
# メッセージの取り出しは、偽の検査を使うユニットテスト（src/rules/check_commit_message.rs）が見る。
#
# 使い方: commit-message.sh <claude-hooks の実行ファイル>
set -euo pipefail

BIN=$1
REPO=$(mktemp -d)
trap 'rm -rf "$REPO" "$REPO.gitconfig"' EXIT
# ルールは、グローバルの core.hooksPath（commit-msg の置き場）を使うリポジトリだけを検査する
export GIT_CONFIG_GLOBAL="$REPO.gitconfig"
git config --global core.hooksPath /nonexistent/global-hooks
git -C "$REPO" init -q

run() {
  jq -n --arg c "$1" --arg d "$REPO" '{tool_name: "Bash", tool_input: {command: $c}, cwd: $d}' |
    "$BIN" pre-tool-use --rule check-commit-message
}

fail() {
  echo "FAIL: $1" >&2
  echo "$2" >&2
  exit 1
}

expect_deny() {
  local what=$1 cmd=$2 needle=$3 out reason
  out=$(run "$cmd")
  [ "$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")" = deny ] || fail "$what を止めるはず" "$out"
  reason=$(jq -r '.hookSpecificOutput.permissionDecisionReason' <<<"$out")
  grep -qF "$needle" <<<"$reason" || fail "$what の理由に「$needle」を示すはず" "$out"
}

expect_pass() {
  local what=$1 cmd=$2 out
  out=$(run "$cmd")
  [ -z "$out" ] || fail "$what は何も出さずに通すはず" "$out"
}

# xlc の記録にある 73 文字の件名（2026-11-06 00:18 JST に commit-msg が拒み、同じ呼び出しの後段の品質チェックが
# 古い HEAD で通ったと出した形）
LONG="docs(plans): say the bank D insertion's effect in Problem 9 is unmeasured"

expect_deny "-F - のヒアドキュメントの 73 文字の件名" "git add docs/plans/m4-depletion.md && git commit -q -F - <<'EOF'
$LONG

本文

Refs: #0190
EOF
git log --oneline -1 && run-checks.sh -C . --base main; echo \"rc=\$?\"" "件名が 72 文字を超えています (73 文字)"

expect_deny "-m のヒアドキュメントの 73 文字の件名" "git commit -m \"\$(cat <<'EOF'
$LONG

本文
EOF
)\"" "件名が 72 文字を超えています (73 文字)"

expect_deny "Conventional Commits でない件名" "git commit -m 'Add the proposed request'" "Conventional Commits 形式ではありません"

expect_pass "72 文字の件名" "git commit -q -F - <<'EOF'
docs(plans): say the bank D insertion's effect in Problem 9 is unmeasure

本文
EOF"

for subject in "Merge: $LONG" "fixup! $LONG" "squash! $LONG" "Revert \"$LONG\""; do
  expect_pass "形式と長さを問わない件名（$subject）" "git commit -F - <<'EOF'
$subject
EOF"
done

echo "ok: check-commit-message"
