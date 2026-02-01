#!/usr/bin/env bash
# gh-pr-review: コメント返信スクリプト
#
# Usage: reply-to-comment.sh <pr_number> <comment_id> <body>
#
# Output: 作成された返信の情報

set -euo pipefail

PR_NUMBER="${1:-}"
COMMENT_ID="${2:-}"
BODY="${3:-}"

if [[ -z "$PR_NUMBER" ]] || [[ -z "$COMMENT_ID" ]] || [[ -z "$BODY" ]]; then
  echo "Usage: $0 <pr_number> <comment_id> <body>" >&2
  exit 1
fi

# リポジトリ情報を取得
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# コメントに返信
gh api "repos/${REPO}/pulls/${PR_NUMBER}/comments/${COMMENT_ID}/replies" \
  -f body="$BODY" \
  --jq '{id: .id, html_url: .html_url, body: .body}'
