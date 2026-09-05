#!/usr/bin/env bash
# PreToolUse hook: 既に再実行済みの run に対する gh run rerun をブロック
#
# 一時障害（GitHub API の 5xx、Copilot の内部エラー）の再実行は 1 回まで。
# attempt 2 以上で同じ失敗が続くなら一時障害ではないので、無制限に再実行せず
# 原因（gh-actions-diagnose.sh の出力）を報告してユーザーの判断に委ねる。
#
# attempt は gh run view <id> --json attempt で確認する。確認できなければ deny する
# （確認できない状態で再実行させない。pre-merge-check.sh と同じ方針）。
#
# エスケープ: コマンドに `ALLOW_RERUN=1` を付ける（guard-git-push.sh と同じ方式）

set -euo pipefail

INPUT=$(cat)
TOOL_NAME=$(jq -r '.tool_name // empty' <<<"$INPUT")
COMMAND=$(jq -r '.tool_input.command // empty' <<<"$INPUT")

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

# ヒアドキュメント本文はデータであってコマンドではない
# shellcheck source=lib/heredoc.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/heredoc.sh"
COMMAND=$(mask_heredoc_bodies <<<"$COMMAND")

DETECT_RE='(^|[[:space:];&|(])gh[[:space:]]+run[[:space:]]+rerun([[:space:]]|$|[;&|])'
grep -qE "$DETECT_RE" <<<"$COMMAND" || exit 0

# エスケープ
if grep -qE '(^|[[:space:];&|(])ALLOW_RERUN=1([[:space:]]|$)' <<<"$COMMAND"; then
  exit 0
fi

deny() {
  jq -n --arg reason "$1"$'\n\n'"どうしても必要な場合は ALLOW_RERUN=1 をコマンドに付けて実行してください。" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $reason
    }
  }'
  exit 0
}

check_rerun() {
  local RERUN_PART="$1"
  local -a REPO_ARGS=()
  local RUN_ID="" tok skip=0
  for tok in $RERUN_PART; do
    if (( skip )); then skip=0; REPO_ARGS+=(-R "$tok"); continue; fi
    case "$tok" in
      -R|--repo) skip=1 ;;
      --repo=*) REPO_ARGS+=(-R "${tok#--repo=}") ;;
      -j|--job) skip=2 ;;   # ジョブ指定は run ID ではない
      -*) ;;
      *) if [[ -z "$RUN_ID" && "$tok" =~ ^[0-9]+$ ]]; then RUN_ID="$tok"; fi ;;
    esac
    # --job の値は読み飛ばすだけ（REPO_ARGS に入れない）
    if (( skip == 2 )); then skip=0; continue; fi
  done
  [[ -n "$RUN_ID" ]] || return 0

  local ATTEMPT
  if ! ATTEMPT=$(gh run view "${REPO_ARGS[@]}" "$RUN_ID" --json attempt 2>/dev/null | jq -r '.attempt // empty') \
     || ! [[ "$ATTEMPT" =~ ^[0-9]+$ ]]; then
    deny "run ${RUN_ID} の試行回数を確認できません（gh run view が失敗）。再実行の前に /gh-actions-check で状況を診断してください"
  fi
  if (( ATTEMPT >= 2 )); then
    deny "run ${RUN_ID} は既に ${ATTEMPT} 回試行して失敗しています。一時障害の再実行は 1 回までです。同じ失敗が続くなら一時障害ではないので、~/.claude/scripts/gh-actions-diagnose.sh の結果（CAUSE / errors）を添えてユーザーに報告し、判断を仰いでください"
  fi
}

# コマンド中の全ての gh run rerun を検査する
REST="$COMMAND"
while [[ "$REST" =~ $DETECT_RE ]]; do
  AFTER="${REST#*"${BASH_REMATCH[0]}"}"
  check_rerun "${AFTER%%[;&|]*}"
  REST="$AFTER"
done

exit 0
