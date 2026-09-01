#!/usr/bin/env bash
set -euo pipefail

# PreToolUse hook: gh pr merge 実行前にマージの前提条件を検証する。
#
# CLAUDE.md の必須ゲートをツールレベルで強制する:
#   1. マージ方式は --merge のみ（--squash / --rebase は禁止）
#   2. --delete-branch でマージ後のブランチを削除する
#   3. マージコミット本文に ## Why / ## What / ## Impact が揃っている
#   4. CI チェックが未完了・失敗していない
#   5. reviewDecision が CHANGES_REQUESTED / REVIEW_REQUIRED でない
#   6. 未解決のレビュースレッドがない
#
# 1〜3 はコマンド文字列だけで判定する。4〜6 は gh で GitHub に問い合わせる。
# 6 は問い合わせに失敗したら deny する（確認できない状態でマージさせない）。

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

# gh pr merge の検出。行頭だけでなく `cd x && gh pr merge` のようにコマンド区切りの
# 後ろにある場合も対象にする（行頭限定だと cd 付きで迂回できてしまう）
DETECT_RE='(^|[[:space:];&|(])gh[[:space:]]+pr[[:space:]]+merge([[:space:]]|$|[;&|])'
if [[ ! "$COMMAND" =~ $DETECT_RE ]]; then
  exit 0
fi

# merge 以降の部分（フラグ解析の対象）
MERGE_PART="${COMMAND#*"${BASH_REMATCH[0]}"}"

# PR番号（merge の直後にある数値。無ければ現在のブランチの PR）
PR_REF=$(echo "$MERGE_PART" | grep -oE '^[[:space:]]*[0-9]+' | tr -d '[:space:]' || true)

REASONS=()

# gh の実行コンテキストをコマンドに合わせる。
# - `cd <path> && gh pr merge` → 最後の cd 先で gh を実行（block-main-commit と同方式）
# - `-R/--repo owner/name`     → そのリポジトリを対象にする
# hook の cwd のまま実行すると、別ディレクトリを対象にしたコマンドで誤った PR を見る
CD_RE='.*(^|&&|;)[[:space:]]*cd[[:space:]]+("[^"]*"|'\''[^'\'']*'\''|[^;&|[:space:]]+)'
if [[ "$COMMAND" =~ $CD_RE ]]; then
  TARGET_DIR="${BASH_REMATCH[2]}"
  TARGET_DIR="${TARGET_DIR%\"}"; TARGET_DIR="${TARGET_DIR#\"}"
  TARGET_DIR="${TARGET_DIR%\'}"; TARGET_DIR="${TARGET_DIR#\'}"
  TARGET_DIR="${TARGET_DIR/#\~/$HOME}"
  if ! cd "$TARGET_DIR" 2>/dev/null; then
    REASONS+=("コマンド中の cd 先に移動できません: ${TARGET_DIR}")
  fi
fi

REPO_ARGS=()
REPO_RE='(^|[[:space:]])(-R|--repo)[[:space:]=]+([^[:space:]]+)'
if [[ "$MERGE_PART" =~ $REPO_RE ]]; then
  REPO_ARGS=(-R "${BASH_REMATCH[3]}")
fi

# フラグの有無（単語境界で判定。-m のような短縮形も許容）
has_flag() {
  local re="(^|[[:space:]])($1)([[:space:]=]|$)"
  [[ "$MERGE_PART" =~ $re ]]
}

# --- 1. マージ方式 ---
if has_flag '--squash|-s|--rebase|-r'; then
  REASONS+=("--squash / --rebase は禁止です。--merge（マージコミット方式）を使ってください")
elif ! has_flag '--merge|-m'; then
  REASONS+=("--merge を明示してください（マージ方式の既定値に依存しない）")
fi

# --- 2. ブランチ削除 ---
if ! has_flag '--delete-branch|-d'; then
  REASONS+=("--delete-branch を付けてください（マージ後のブランチは速やかに削除する）")
fi

# --- 3. 本文形式 ---
BODY_TEXT="$MERGE_PART"
BODY_FILE=$(echo "$MERGE_PART" | grep -oE '(--body-file|-F)[[:space:]=]+[^[:space:]]+' | head -n 1 | sed -E 's/^(--body-file|-F)[[:space:]=]+//' || true)
if [[ -n "$BODY_FILE" && -r "$BODY_FILE" ]]; then
  BODY_TEXT=$(cat "$BODY_FILE")
fi
if ! has_flag '--body|-b|--body-file|-F'; then
  REASONS+=("--body でマージコミット本文を指定してください（## Why / ## What / ## Impact）")
else
  MISSING=()
  for h in '## Why' '## What' '## Impact'; do
    grep -qF -- "$h" <<<"$BODY_TEXT" || MISSING+=("$h")
  done
  if [[ ${#MISSING[@]} -gt 0 ]]; then
    REASONS+=("マージコミット本文に見出しがありません: ${MISSING[*]}")
  fi
fi

# --- 4. CI チェック ---
CHECK_ARGS=("${REPO_ARGS[@]}")
if [[ -n "${PR_REF:-}" ]]; then
  CHECK_ARGS+=("$PR_REF")
fi

CHECKS=$(gh pr checks "${CHECK_ARGS[@]}" --json name,state,status 2>/dev/null || echo '[]')

if [[ "$(echo "$CHECKS" | jq 'length')" -gt 0 ]]; then
  PENDING=$(echo "$CHECKS" | jq '[.[] | select(.status != "COMPLETED")] | length')
  if [[ "$PENDING" -gt 0 ]]; then
    PENDING_NAMES=$(echo "$CHECKS" | jq -r '[.[] | select(.status != "COMPLETED") | .name] | join(", ")')
    REASONS+=("実行中/待機中のチェックがあります (${PENDING}件): ${PENDING_NAMES}")
  fi

  FAILED=$(echo "$CHECKS" | jq '[.[] | select(.status == "COMPLETED" and .state != "SUCCESS" and .state != "NEUTRAL" and .state != "SKIPPED")] | length')
  if [[ "$FAILED" -gt 0 ]]; then
    FAILED_NAMES=$(echo "$CHECKS" | jq -r '[.[] | select(.status == "COMPLETED" and .state != "SUCCESS" and .state != "NEUTRAL" and .state != "SKIPPED") | "\(.name) (\(.state))"] | join(", ")')
    REASONS+=("失敗したチェックがあります (${FAILED}件): ${FAILED_NAMES}")
  fi
fi

# --- 5. レビュー判定 ---
REVIEW_DECISION=$(gh pr view "${CHECK_ARGS[@]}" --json reviewDecision --jq '.reviewDecision' 2>/dev/null || true)

if [[ "$REVIEW_DECISION" == "CHANGES_REQUESTED" ]]; then
  REASONS+=("レビューで変更が要求されています (CHANGES_REQUESTED)")
fi

if [[ "$REVIEW_DECISION" == "REVIEW_REQUIRED" ]]; then
  REASONS+=("必須レビューが未完了です (REVIEW_REQUIRED)")
fi

# --- 6. 未解決レビュースレッド ---
# Copilot のレビューは COMMENTED で提出され reviewDecision を変えないため、
# 指摘への対応漏れはスレッドの resolve 状態で判定する（対応後は
# gh-pr-review の resolve-thread.sh で resolve する運用）
PR_NUMBER="${PR_REF:-}"
if [[ -z "$PR_NUMBER" ]]; then
  PR_NUMBER=$(gh pr view "${REPO_ARGS[@]}" --json number --jq '.number' 2>/dev/null || true)
fi
if [[ -z "$PR_NUMBER" ]]; then
  REASONS+=("対象 PR を特定できません（PR番号を指定するか、PR のあるブランチで実行してください）")
else
  OWNER=$(gh repo view "${REPO_ARGS[@]}" --json owner --jq '.owner.login' 2>/dev/null || true)
  NAME=$(gh repo view "${REPO_ARGS[@]}" --json name --jq '.name' 2>/dev/null || true)
  UNRESOLVED=$(gh api graphql -F owner="$OWNER" -F name="$NAME" -F number="$PR_NUMBER" -f query='
    query($owner: String!, $name: String!, $number: Int!) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          reviewThreads(first: 100) {
            nodes { isResolved path }
          }
        }
      }
    }' --jq '[.data.repository.pullRequest.reviewThreads.nodes[] | select(.isResolved | not) | .path]' 2>/dev/null || true)
  if [[ -z "$UNRESOLVED" ]]; then
    REASONS+=("未解決レビュースレッドを確認できませんでした（gh api graphql が失敗）")
  else
    UNRESOLVED_COUNT=$(jq 'length' <<<"$UNRESOLVED")
    if [[ "$UNRESOLVED_COUNT" -gt 0 ]]; then
      UNRESOLVED_PATHS=$(jq -r 'unique | join(", ")' <<<"$UNRESOLVED")
      REASONS+=("未解決のレビュースレッドがあります (${UNRESOLVED_COUNT}件): ${UNRESOLVED_PATHS}。対応して返信し、resolve-thread.sh で resolve してください")
    fi
  fi
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

# 全ゲート通過 → マージ許可
exit 0
