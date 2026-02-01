#!/usr/bin/env bash
# gh-pr-review: PR情報取得スクリプト
#
# Usage: get-pr-info.sh [pr_number]
#
# pr_number省略時: 現在のブランチに関連するPRを取得
# Output: JSON形式のPR情報

set -euo pipefail

PR_NUMBER="${1:-}"

if [[ -z "$PR_NUMBER" ]]; then
  # 現在のブランチに関連するPRを取得
  gh pr view --json number,title,url,state,reviewDecision,headRefName,baseRefName
else
  gh pr view "$PR_NUMBER" --json number,title,url,state,reviewDecision,headRefName,baseRefName
fi
