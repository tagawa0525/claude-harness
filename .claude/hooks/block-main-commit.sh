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

# 正規表現の部品（\b や \s は GNU 拡張のため POSIX 文字クラスで境界を表現）
# OPT: 「-」で始まるオプション。引数を1つ取る形（-C <path>, -c key=val 等）にも対応
# PATH_TOKEN: "..." / '...' のクォート付き、または裸のパス
OPT='-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?'
PATH_TOKEN='"[^"]*"|'\''[^'\'']*'\''|[^[:space:]]+'

# git commit コマンドかどうか判定
# git と commit の間はオプションのみ許可し、`git log --grep commit` のような
# 誤検出を防ぐ
DETECT_RE='(^|[[:space:];&|(])git[[:space:]]+(('"$OPT"')[[:space:]]+)*commit([[:space:]]|$|[;&|])'
if ! echo "$TOOL_INPUT" | grep -qE "$DETECT_RE"; then
  exit 0
fi

# コマンドが対象とするディレクトリを推定（worktree での誤判定を防ぐ）
# - git -C <path> commit    → <path>（commit と同一の git 呼び出しに限る）
# - cd <path> && git commit → <path>（複数あれば最後の cd を採用）
# - それ以外                → hook の cwd
GIT_C_RE='git[[:space:]]+-C[[:space:]]+('"$PATH_TOKEN"')([[:space:]]+'"$OPT"')*[[:space:]]+commit'
CD_RE='.*(^|&&|;)[[:space:]]*cd[[:space:]]+("[^"]*"|'\''[^'\'']*'\''|[^;&|[:space:]]+)'
TARGET_DIR="."
if [[ "$TOOL_INPUT" =~ $GIT_C_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[1]}"
elif [[ "$TOOL_INPUT" =~ $CD_RE ]]; then
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
