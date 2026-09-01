#!/usr/bin/env bash
# PreToolUse hook: 長時間待機するスクリプトのフォアグラウンド実行をブロックする
#
# gh-wait-review.sh / request-rereview.sh は漸増バックオフで約10分待つため、
# Bash ツールのフォアグラウンド上限を必ず超えて失敗する。run_in_background=true
# でのみ実行を許可する（CLAUDE.md「必ずバックグラウンドで実行する」の強制）。
# 回避する正当な理由がないため、エスケープは設けない。

set -euo pipefail

INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
IN_BACKGROUND=$(echo "$INPUT" | jq -r '.tool_input.run_in_background // false')

[[ "$TOOL_NAME" == "Bash" ]] || exit 0

if ! echo "$COMMAND" | grep -qE '(gh-wait-review|request-rereview)\.sh'; then
  exit 0
fi

# ヘルプ表示や構文チェックなど、待機しない呼び出しは対象外
if echo "$COMMAND" | grep -qE '(^|[[:space:]])(bash[[:space:]]+-n|shellcheck|cat|head|tail|sed|grep|less)[[:space:]]'; then
  exit 0
fi

if [[ "$IN_BACKGROUND" == "true" ]]; then
  exit 0
fi

jq -n '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: "gh-wait-review.sh / request-rereview.sh は約10分待機するため、Bash ツールの run_in_background=true で実行してください（フォアグラウンドでは上限に達して必ず失敗します）"
  }
}'
exit 0
