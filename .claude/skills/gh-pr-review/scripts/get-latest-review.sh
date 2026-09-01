#!/usr/bin/env bash
# gh-pr-review: 最新の Copilot レビューの要約を取得するスクリプト
#
# Usage: get-latest-review.sh <pr_number>
#
# 出力（テキスト）:
#   ROUND: <n>              PR 上の Copilot レビュー件数 = 現在の周回数
#   REVIEW_ID: <id>
#   STATE: <state>
#   HEADLINE: <本文1行目>    例: "### 🟢 Approval recommended"
#   INLINE_COMMENTS: <n>    このレビューに紐づくインラインコメント数（通常の指摘）
#   SUPPRESSED_COMMENTS: <n>
#   --- suppressed ---      以降、Suppressed comments セクションの本文
#
# Suppressed comments は Copilot が低確度と判断した指摘で、インラインスレッドには
# ならずレビュー本文の <details> 内にだけ現れる。get-review-comments.sh では
# 取得できないため、このスクリプトで別途読む。

set -euo pipefail

PR_NUMBER="${1:-}"

if [[ -z "$PR_NUMBER" ]]; then
  echo "Usage: $0 <pr_number>" >&2
  exit 1
fi

REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# Copilot のレビューを提出順に全件取得（周回数の算出に全件必要）
reviews=$(gh api --paginate "repos/${REPO}/pulls/${PR_NUMBER}/reviews?per_page=100" \
  --jq '[.[] | select(.user.login == "copilot-pull-request-reviewer[bot]")]')

round=$(jq 'length' <<<"$reviews")
if (( round == 0 )); then
  echo "ROUND: 0"
  echo "NOTE: Copilot のレビューはまだありません"
  exit 0
fi

latest=$(jq '.[-1]' <<<"$reviews")
review_id=$(jq -r '.id' <<<"$latest")
state=$(jq -r '.state' <<<"$latest")
body=$(jq -r '.body' <<<"$latest")

inline_count=$(gh api --paginate \
  "repos/${REPO}/pulls/${PR_NUMBER}/reviews/${review_id}/comments?per_page=100" \
  --jq 'length' | awk '{s+=$1} END {print s+0}')

# "### Suppressed comments (N)" から次の </details> までを抜き出す
suppressed=$(awk '
  /^### Suppressed comments/ { on=1 }
  on && (/<\/details>/ || /^- \*\*Files reviewed:/) { exit }
  on { print }
' <<<"$body")
suppressed_count=$(grep -o -E '^### Suppressed comments \([0-9]+\)' <<<"$suppressed" \
  | grep -o -E '[0-9]+' || echo 0)

echo "ROUND: ${round}"
echo "REVIEW_ID: ${review_id}"
echo "STATE: ${state}"
echo "HEADLINE: $(head -n 1 <<<"$body")"
echo "INLINE_COMMENTS: ${inline_count}"
echo "SUPPRESSED_COMMENTS: ${suppressed_count}"
if (( suppressed_count > 0 )); then
  echo "--- suppressed ---"
  echo "$suppressed"
fi
