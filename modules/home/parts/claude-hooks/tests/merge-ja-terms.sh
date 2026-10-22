#!/usr/bin/env bash
# nix でビルドした claude-hooks の check-merge-body-ja-terms が、埋めた ja-terms-message と辞書（ja-terms の
# prh.yml）で git merge の本文の崩れた語を止め、正しい語と、commit-msg が見ない Merge の件名を通すことを
# 確かめる（package.nix の installCheckPhase が呼ぶ）。本文の取り出しは、偽の ja-terms-message を使う
# ユニットテスト（src/rules/check_merge_body_ja_terms.rs）が見る。
# 崩れた語をわざと含むので、語の検査から外す（.textlintignore）
#
# 使い方: merge-ja-terms.sh <claude-hooks の実行ファイル>
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
    "$BIN" pre-tool-use --rule check-merge-body-ja-terms
}

fail() {
  echo "FAIL: $1" >&2
  echo "$2" >&2
  exit 1
}

out=$(run "git merge --no-ff feat/a -m \"\$(cat <<'EOF'
Merge: a

## Why
掃引して計算を流し直す
EOF
)\"")
decision=$(jq -r '.hookSpecificOutput.permissionDecision // empty' <<<"$out")
[ "$decision" = deny ] || fail "崩れた語を止めるはず" "$out"
reason=$(jq -r '.hookSpecificOutput.permissionDecisionReason' <<<"$out")
for term in "掃引 => スイープ" "流し直す => 再実行する"; do
  grep -qF "$term" <<<"$reason" || fail "「$term」を示すはず" "$out"
done

out=$(run "git merge --no-ff feat/a -m 'Merge: a' -m '確認の結果を再実行する'")
[ -z "$out" ] || fail "正しい語は何も出さずに通すはず" "$out"

out=$(run "git merge --no-ff feat/a -m 'Merge: 掃引' -m '確認'")
[ -z "$out" ] || fail "commit-msg と同じく Merge の件名は検査しないはず" "$out"

echo "ok: check-merge-body-ja-terms"
