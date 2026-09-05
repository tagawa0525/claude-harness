#!/usr/bin/env bash
# PreToolUse hook: 生の gh api で行ってはいけない操作をブロック
#
# 1. Actions の権限設定（repos/.../actions/permissions 配下）への書き込み
#    CI の権限エラー（Resource not accessible by integration 等）は、ワークフローの
#    permissions: に必要な権限を最小限で宣言して直す。リポジトリ設定
#    default_workflow_permissions を write に緩めるのは回避策であり、全ワークフローの
#    権限を広げるので禁止する。読み取り（GET）は通す
# 2. GraphQL の resolveReviewThread / unresolveReviewThread
#    スレッドの resolve は gh-pr-review の resolve-thread.sh 経由に固定する。
#    スクリプトは人間のレビュアーが起こしたスレッドを既定で拒否するので、
#    生の mutation を許すとその判定を迂回できてしまう
#
# 1 はコマンド文字列（ヒアドキュメント本文をマスクしたもの）で判定する。
# 2 は mutation の本文そのものが操作なので、ヒアドキュメントで渡された場合も
# 元の文字列で検査する（マスク後の検出位置を元の文字列に適用する。README 参照）。
# エスケープはない（どちらも Claude Code セッションから行う理由がない）

set -euo pipefail

INPUT=$(cat)
TOOL_NAME=$(jq -r '.tool_name // empty' <<<"$INPUT")
COMMAND=$(jq -r '.tool_input.command // empty' <<<"$INPUT")

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

# shellcheck source=lib/heredoc.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
COMMAND_RAW="$COMMAND"
COMMAND=$(mask_heredoc_bodies <<<"$COMMAND")

DETECT_RE='(^|[[:space:];&|(])gh[[:space:]]+api([[:space:]]|$|[;&|])'
grep -qE "$DETECT_RE" <<<"$COMMAND" || exit 0

deny() {
  jq -n --arg reason "$1" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
}

check_api() {
  local API_PART="$1" API_PART_RAW="$2"

  # --- 1. actions/permissions への書き込み ---
  if grep -qE 'actions/permissions' <<<"$API_PART"; then
    local writes=0
    if grep -qE '(^|[[:space:]])(-X|--method)[[:space:]=]+(PUT|PATCH|POST|DELETE)([[:space:]]|$)' <<<"$API_PART"; then
      writes=1
    fi
    # フィールドや入力ファイルを渡すと gh api は既定で POST になる
    if grep -qE '(^|[[:space:]])(-f|-F|--field|--raw-field|--input)([[:space:]=]|$)' <<<"$API_PART"; then
      writes=1
    fi
    if (( writes )); then
      deny "リポジトリの Actions 権限設定（actions/permissions、default_workflow_permissions 等）は変更しません。全ワークフローの権限を広げる回避策になるためです。CI の権限エラーは、そのワークフローの permissions: に必要な権限を最小限で宣言して直してください（例: pull-requests: write）"
    fi
  fi

  # --- 2. resolveReviewThread ---
  if grep -qE '(^|[[:space:]])graphql([[:space:]]|$)' <<<"$API_PART" \
     && grep -qE '(^|[^A-Za-z])(un)?resolveReviewThread' <<<"$API_PART_RAW"; then
    deny "レビュースレッドの resolve / unresolve は生の GraphQL では行いません。~/.claude/skills/gh-pr-review/scripts/resolve-thread.sh <pr_number> <comment_id> を使ってください（人間のレビュアーが起こしたスレッドは既定で resolve しない判定をスクリプトが行います）"
  fi
}

# コマンド中の全ての gh api を検査する。マスクは長さを保つので、
# マスク後で見つけた位置と長さをそのまま元の文字列に適用できる
REST="$COMMAND"
REST_RAW="$COMMAND_RAW"
while [[ "$REST" =~ $DETECT_RE ]]; do
  PREFIX="${REST%%"${BASH_REMATCH[0]}"*}"
  OFFSET=$(( ${#PREFIX} + ${#BASH_REMATCH[0]} ))
  AFTER="${REST:OFFSET}"
  AFTER_RAW="${REST_RAW:OFFSET}"
  PART="${AFTER%%[;&|]*}"
  check_api "$PART" "${AFTER_RAW:0:${#PART}}"
  REST="$AFTER"
  REST_RAW="$AFTER_RAW"
done

exit 0
