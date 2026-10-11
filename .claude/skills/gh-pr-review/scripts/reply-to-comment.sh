#!/usr/bin/env bash
# gh-pr-review: コメント返信スクリプト
#
# Usage: reply-to-comment.sh <PR の URL> <comment_id> <body>
#
# Output: 作成された返信の情報

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/pr-url.sh"
COMMENT_ID="${2:-}"
BODY="${3:-}"

if ! parse_pr_url "${1:-}" || [[ -z "$COMMENT_ID" ]] || [[ -z "$BODY" ]]; then
  echo "Usage: $0 <PR の URL> <comment_id> <body>" >&2
  exit 1
fi

# コメントに返信
gh api "repos/{owner}/{repo}/pulls/${PR_NUMBER}/comments/${COMMENT_ID}/replies" \
  -f body="$BODY" \
  --jq '{id: .id, html_url: .html_url, body: .body}'
