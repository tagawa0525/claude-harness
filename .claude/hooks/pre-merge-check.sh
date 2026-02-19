#!/usr/bin/env bash
set -euo pipefail

# PreToolUse hook: gh pr merge 実行前にチェック状態とレビュー状態を検証する。
# 未完了のチェックや失敗したチェック、未完了のレビューがある場合はマージをブロックする。

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

# gh pr merge 以外は素通し
if [[ ! "$COMMAND" =~ ^[[:space:]]*gh[[:space:]]+pr[[:space:]]+merge ]]; then
  exit 0
fi

# PR番号を抽出（gh pr merge の直後にある数値）
PR_REF=$(echo "$COMMAND" | sed -E 's/^[[:space:]]*gh[[:space:]]+pr[[:space:]]+merge[[:space:]]*//' | grep -oE '^[0-9]+' || true)

# --- チェック状態の確認 ---
CHECK_ARGS=()
if [[ -n "${PR_REF:-}" ]]; then
  CHECK_ARGS+=("$PR_REF")
fi

CHECKS=$(gh pr checks "${CHECK_ARGS[@]}" --json name,state,status 2>/dev/null) || exit 0

REASONS=()

# チェックが存在する場合のみ検証
if [[ "$(echo "$CHECKS" | jq 'length')" -gt 0 ]]; then
  # 未完了のチェック（in_progress, queued, etc.）
  PENDING=$(echo "$CHECKS" | jq '[.[] | select(.status != "COMPLETED")] | length')
  if [[ "$PENDING" -gt 0 ]]; then
    PENDING_NAMES=$(echo "$CHECKS" | jq -r '[.[] | select(.status != "COMPLETED") | .name] | join(", ")')
    REASONS+=("実行中/待機中のチェックがあります (${PENDING}件): ${PENDING_NAMES}")
  fi

  # 失敗したチェック（completed だが success/neutral/skipped 以外）
  FAILED=$(echo "$CHECKS" | jq '[.[] | select(.status == "COMPLETED" and .state != "SUCCESS" and .state != "NEUTRAL" and .state != "SKIPPED")] | length')
  if [[ "$FAILED" -gt 0 ]]; then
    FAILED_NAMES=$(echo "$CHECKS" | jq -r '[.[] | select(.status == "COMPLETED" and .state != "SUCCESS" and .state != "NEUTRAL" and .state != "SKIPPED") | "\(.name) (\(.state))"] | join(", ")')
    REASONS+=("失敗したチェックがあります (${FAILED}件): ${FAILED_NAMES}")
  fi
fi

# --- レビュー状態の確認 ---
REVIEW_DECISION=$(gh pr view "${CHECK_ARGS[@]}" --json reviewDecision --jq '.reviewDecision' 2>/dev/null || true)

if [[ "$REVIEW_DECISION" == "CHANGES_REQUESTED" ]]; then
  REASONS+=("レビューで変更が要求されています (CHANGES_REQUESTED)")
fi

if [[ "$REVIEW_DECISION" == "REVIEW_REQUIRED" ]]; then
  REASONS+=("必須レビューが未完了です (REVIEW_REQUIRED)")
fi

# --- 結果出力 ---
if [[ ${#REASONS[@]} -gt 0 ]]; then
  REASON_TEXT=$(printf '%s\n' "${REASONS[@]}")
  REASON_TEXT="${REASON_TEXT}"$'\n\n'"/gh-actions-check で状況を診断してください。"

  jq -n --arg reason "$REASON_TEXT" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
fi

# 全チェック通過・レビュー問題なし → マージ許可
exit 0
