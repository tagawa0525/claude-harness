#!/usr/bin/env bash
# PreToolUse hook: mainブランチでの git commit をブロック
#
# CLAUDE.md の「mainブランチに直接コミットしない」ルールをツールレベルで強制。
# スキル経由でない直接的な git commit も捕捉する。

set -euo pipefail

# stdin から PreToolUse フックの入力を読み取る
INPUT=$(cat)

TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
TOOL_INPUT=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

# Bash ツールの git commit コマンドのみ対象
if [[ "$TOOL_NAME" != "Bash" ]]; then
  exit 0
fi

# git commit コマンドかどうか判定（git commit で始まるコマンドを検出）
if ! echo "$TOOL_INPUT" | grep -qE '(^|\b|&&\s*|;\s*)git commit\b'; then
  exit 0
fi

# 現在のブランチを取得
CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "")

# main または master ブランチの場合はブロック
if [[ "$CURRENT_BRANCH" == "main" || "$CURRENT_BRANCH" == "master" ]]; then
  cat <<RESULT
{
  "decision": "block",
  "reason": "mainブランチへの直接コミットは禁止されています。featureブランチを作成してください: /git-branch"
}
RESULT
  exit 0
fi
