#!/usr/bin/env bash
# gh-pr-review: PR情報取得スクリプト
#
# Usage: get-pr-info.sh [pr_number | PR URL | コメント URL]
#
# 省略時: 現在のブランチに関連するPRを取得
# URL:    https://github.com/{owner}/{repo}/pull/{n}[#discussion_r{comment_id}]
#         owner/repo を -R で指定して取得し、comment_id（なければ null）を添える
# Output: JSON形式のPR情報

set -euo pipefail

ARG="${1:-}"
FIELDS=number,title,url,state,reviewDecision,headRefName,baseRefName
URL_RE='^https://github\.com/([^/]+)/([^/]+)/pull/([0-9]+)/?(#discussion_r([0-9]+))?$'

if [[ -z "$ARG" ]]; then
  gh pr view --json "$FIELDS"
elif [[ "$ARG" =~ $URL_RE ]]; then
  OWNER="${BASH_REMATCH[1]}"
  REPO="${BASH_REMATCH[2]}"
  NUMBER="${BASH_REMATCH[3]}"
  COMMENT_ID="${BASH_REMATCH[5]}"
  gh pr view "$NUMBER" -R "$OWNER/$REPO" --json "$FIELDS" \
    | jq --arg cid "$COMMENT_ID" '. + {comment_id: (if $cid == "" then null else ($cid | tonumber) end)}'
else
  gh pr view "$ARG" --json "$FIELDS"
fi
