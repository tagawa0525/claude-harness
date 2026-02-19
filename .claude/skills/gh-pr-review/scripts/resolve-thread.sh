#!/usr/bin/env bash
# gh-pr-review: スレッド解決スクリプト
#
# Usage: resolve-thread.sh <pr_number> <thread_node_id>
#
# Note: thread_node_id は GraphQL の node_id（例: PRRT_xxx）

set -euo pipefail

PR_NUMBER="${1:-}"
THREAD_NODE_ID="${2:-}"

if [[ -z "$PR_NUMBER" ]] || [[ -z "$THREAD_NODE_ID" ]]; then
  echo "Usage: $0 <pr_number> <thread_node_id>" >&2
  exit 1
fi

# リポジトリ情報を取得
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# gh pr-review 拡張が利用可能かチェック
if gh extension list | grep -q 'pr-review'; then
  gh pr-review threads resolve "$PR_NUMBER" -R "$REPO" \
    --thread-id "$THREAD_NODE_ID"
else
  # GraphQL API でスレッドを解決
  gh api graphql \
    -f threadId="$THREAD_NODE_ID" \
    -f query='
    mutation($threadId: ID!) {
      resolveReviewThread(input: {threadId: $threadId}) {
        thread {
          id
          isResolved
        }
      }
    }
  ' --jq '.data.resolveReviewThread.thread'
fi
