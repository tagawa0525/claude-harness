#!/usr/bin/env bash
# gh-pr-review: PR情報取得スクリプト
#
# Usage: get-pr-info.sh <PR の URL | コメントの URL>
#
# URL: https://github.com/{owner}/{repo}/pull/{n}[#discussion_r{comment_id}]
#      comment_id（なければ null）を PR 情報に添える
# Output: JSON形式のPR情報

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/pr-url.sh"
if ! parse_pr_url "${1:-}"; then
  echo "Usage: $0 <PR の URL | コメントの URL>" >&2
  exit 1
fi

COMMENT_ID=""
if [[ -n "$PR_URL_FRAGMENT" ]]; then
  if [[ ! "$PR_URL_FRAGMENT" =~ ^discussion_r([0-9]+)$ ]]; then
    echo "ERROR: URL の # 以降はレビューコメント（#discussion_r<id>）だけを受けます: '#${PR_URL_FRAGMENT}'" >&2
    exit 1
  fi
  COMMENT_ID="${BASH_REMATCH[1]}"
fi

gh pr view "$PR_NUMBER" --json number,title,url,state,reviewDecision,headRefName,baseRefName \
  | jq --arg cid "$COMMENT_ID" '. + {comment_id: (if $cid == "" then null else ($cid | tonumber) end)}'
