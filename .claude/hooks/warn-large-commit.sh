#!/usr/bin/env bash
# PreToolUse hook: 大規模なコミットの件数をモデルに伝える（ブロックしない）
#
# git-commit スキルの「大規模変更の警告」（5 ファイル以上または 100 行以上）を
# hook に移したもの。数えるのは決定的なので hook が行い、「1 つの論理的変更に
# 収まっているか」「分割するか」の判断はモデルに残す。そのため deny ではなく
# additionalContext で件数と確認事項だけを渡す。

set -euo pipefail

INPUT=$(cat)
TOOL_NAME=$(jq -r '.tool_name // empty' <<<"$INPUT")
COMMAND=$(jq -r '.tool_input.command // empty' <<<"$INPUT")

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

# ヒアドキュメント本文はデータであってコマンドではない
# （ドキュメントやコミットメッセージに書かれたコマンド例に反応しないため）
# shellcheck source=lib/heredoc.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
COMMAND=$(mask_heredoc_bodies <<<"$COMMAND")

# 正規表現の部品（block-main-commit.sh と同じ）
OPT='-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?'
PATH_TOKEN='"[^"]*"|'\''[^'\'']*'\''|[^[:space:]]+'

DETECT_RE='(^|[[:space:];&|(])git[[:space:]]+(('"$OPT"')[[:space:]]+)*commit([[:space:]]|$|[;&|])'
[[ "$COMMAND" =~ $DETECT_RE ]] || exit 0

# commit 以降（-a の検出対象。次のコマンド区切りまで）
COMMIT_PART="${COMMAND#*"${BASH_REMATCH[0]}"}"
COMMIT_PART="${COMMIT_PART%%[;&|]*}"

# 対象ディレクトリ（-C / 最後の cd）
GIT_C_RE='git[[:space:]]+-C[[:space:]]+('"$PATH_TOKEN"')([[:space:]]+'"$OPT"')*[[:space:]]+commit'
CD_RE='.*(^|&&|;)[[:space:]]*cd[[:space:]]+("[^"]*"|'\''[^'\'']*'\''|[^;&|[:space:]]+)'
TARGET_DIR="."
if [[ "$COMMAND" =~ $GIT_C_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[1]}"
elif [[ "$COMMAND" =~ $CD_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[2]}"
fi
TARGET_DIR="${TARGET_DIR%\"}"; TARGET_DIR="${TARGET_DIR#\"}"
TARGET_DIR="${TARGET_DIR%\'}"; TARGET_DIR="${TARGET_DIR#\'}"
TARGET_DIR="${TARGET_DIR/#\~/$HOME}"

# -a / --all（-am のようなクラスタも）ならワーキングツリーの変更も含めて数える。
# `--amend` / `--author` は `--` で始まるので短縮形のパターンには当たらない
ALL_RE='(^|[[:space:]])(--all|-[a-zA-Z]*a[a-zA-Z]*)([[:space:]]|$)'
if [[ "$COMMIT_PART" =~ $ALL_RE ]]; then
  NUMSTAT=$(git -C "$TARGET_DIR" diff HEAD --numstat 2>/dev/null || true)
else
  NUMSTAT=$(git -C "$TARGET_DIR" diff --cached --numstat 2>/dev/null || true)
fi
[[ -n "$NUMSTAT" ]] || exit 0

# numstat: 追加\t削除\tパス（バイナリは "-" なので 0 扱い）
read -r FILES LINES < <(awk '
  { files++; if ($1 ~ /^[0-9]+$/) lines += $1; if ($2 ~ /^[0-9]+$/) lines += $2 }
  END { printf "%d %d\n", files, lines }' <<<"$NUMSTAT")

if (( FILES < 5 && LINES < 100 )); then
  exit 0
fi

jq -n --arg ctx "⚠️ 大規模な変更です（${FILES} ファイル、${LINES} 行）。1 つの論理的変更に収まっているか確認してください。複数の変更が混在していれば git add でファイル単位（または git add -p で部分単位）に分け、別々にコミットしてください。大きくても 1 つの変更ならそのまま続行してよいです。" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    additionalContext: $ctx
  }
}'
exit 0
