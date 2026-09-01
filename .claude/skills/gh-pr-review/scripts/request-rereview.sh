#!/usr/bin/env bash
# gh-pr-review: Copilot に再レビューを依頼し、その応答を待つスクリプト
#
# Usage: request-rereview.sh <pr_number> [commit_hash ...]
#
# 1. "@copilot 指摘に対応しました (hashes)。再レビューをお願いします。" を PR に投稿
# 2. 投稿されたコメントの created_at（GitHub 側の時刻）を基準に gh-wait-review.sh で待機
#
# 依頼コメントの投稿と待機開始の間に Copilot が応答すると、起動時点を基準にする
# 待機では取りこぼして約10分タイムアウトしていた。依頼コメント自身の時刻を基準に
# することでこの競合を構造的に無くす。
#
# 約10分待つため、Bash ツールでは run_in_background=true で実行すること。
# 終了コードは gh-wait-review.sh に従う（0=応答到着, 1=タイムアウト）。

set -euo pipefail

PR_NUMBER="${1:-}"
if [[ -z "$PR_NUMBER" ]]; then
  echo "Usage: $0 <pr_number> [commit_hash ...]" >&2
  exit 1
fi
if ! [[ "$PR_NUMBER" =~ ^[0-9]+$ ]]; then
  echo "ERROR: pr_number は数値で指定してください: '${PR_NUMBER}'" >&2
  exit 1
fi
shift

if (( $# > 0 )); then
  # "$*" の区切りは IFS の先頭1文字だけなので ", " にはならない。printf で明示的に join する
  hashes=$(printf '%s, ' "$@")
  hashes=${hashes%, }
  body="@copilot 指摘に対応しました (${hashes})。再レビューをお願いします。"
else
  body="@copilot 指摘に対応しました。再レビューをお願いします。"
fi

REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# gh pr comment は URL しか返さないため、REST で投稿して created_at を受け取る
created_at=$(gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" -f body="$body" --jq '.created_at')
echo "POSTED: ${body}"
echo "SINCE: ${created_at}"

exec "$HOME/.claude/scripts/gh-wait-review.sh" "$PR_NUMBER" --since "$created_at"
