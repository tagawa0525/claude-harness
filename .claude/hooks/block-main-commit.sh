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

# git commit コマンドかどうか判定（git -C <path> commit も捕捉）
if ! echo "$TOOL_INPUT" | grep -qE '(^|\b|&&\s*|;\s*)git\s+(-C\s+\S+\s+)?commit\b'; then
  exit 0
fi

# コマンドが対象とするディレクトリを推定（worktree での誤判定を防ぐ）
# - git -C <path> commit    → <path>
# - cd <path> && git commit → <path>
# - それ以外                → hook の cwd
TARGET_DIR="."
if [[ "$TOOL_INPUT" =~ git[[:space:]]+-C[[:space:]]+([^[:space:]]+) ]]; then
  TARGET_DIR="${BASH_REMATCH[1]}"
elif [[ "$TOOL_INPUT" =~ (^|\&\&|\;)[[:space:]]*cd[[:space:]]+([^[:space:]\;\&\|]+) ]]; then
  TARGET_DIR="${BASH_REMATCH[2]}"
fi
# クォート除去とチルダ展開
TARGET_DIR="${TARGET_DIR%\"}"
TARGET_DIR="${TARGET_DIR#\"}"
TARGET_DIR="${TARGET_DIR%\'}"
TARGET_DIR="${TARGET_DIR#\'}"
TARGET_DIR="${TARGET_DIR/#\~/$HOME}"

# 対象ディレクトリのブランチを取得
CURRENT_BRANCH=$(git -C "$TARGET_DIR" branch --show-current 2>/dev/null || echo "")

# main または master ブランチの場合はブロック
if [[ "$CURRENT_BRANCH" == "main" || "$CURRENT_BRANCH" == "master" ]]; then
  jq -n '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: "mainブランチへの直接コミットは禁止されています。featureブランチを作成してください: /git-branch"
    }
  }'
  exit 0
fi
