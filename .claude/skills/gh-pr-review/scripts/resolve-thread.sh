#!/usr/bin/env bash
# gh-pr-review: レビュースレッドを resolve するスクリプト
#
# Usage: resolve-thread.sh <pr_number> <comment_id>
#
#   comment_id: スレッド内の任意のレビューコメントの数値 ID
#               （get-review-comments.sh の id / URL の #discussion_r{id}）
#
# REST には resolve API がないため GraphQL resolveReviewThread を使う。
# 対応済みスレッドを返信後に resolve することで、bot の再レビュー自動化の
# トリガーになるリポジトリでも整合が取れる。

set -euo pipefail

PR_NUMBER="${1:-}"
COMMENT_ID="${2:-}"

if [[ -z "$PR_NUMBER" ]] || [[ -z "$COMMENT_ID" ]]; then
  echo "Usage: $0 <pr_number> <comment_id>" >&2
  exit 1
fi

OWNER=$(gh repo view --json owner -q '.owner.login')
NAME=$(gh repo view --json name -q '.name')

# コメント ID を含むスレッドを探す（スレッド 100 件・スレッド内コメント 100 件まで）
threads=$(gh api graphql \
  -F owner="$OWNER" -F name="$NAME" -F number="$PR_NUMBER" \
  -f query='
    query($owner: String!, $name: String!, $number: Int!) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          reviewThreads(first: 100) {
            pageInfo { hasNextPage }
            nodes {
              id
              isResolved
              comments(first: 100) { nodes { databaseId } }
            }
          }
        }
      }
    }' --jq '.data.repository.pullRequest.reviewThreads')

if [[ "$(jq -r '.pageInfo.hasNextPage' <<<"$threads")" == "true" ]]; then
  echo "ERROR: レビュースレッドが 100 件を超えています（未対応）" >&2
  exit 1
fi

thread=$(jq --argjson cid "$COMMENT_ID" \
  '.nodes[] | select(any(.comments.nodes[]; .databaseId == $cid))' <<<"$threads")

if [[ -z "$thread" ]]; then
  echo "ERROR: コメント ${COMMENT_ID} を含むスレッドが PR #${PR_NUMBER} に見つかりません" >&2
  exit 1
fi

thread_id=$(jq -r '.id' <<<"$thread")

if [[ "$(jq -r '.isResolved' <<<"$thread")" == "true" ]]; then
  echo "already resolved: ${thread_id}"
  exit 0
fi

gh api graphql -F id="$thread_id" \
  -f query='
    mutation($id: ID!) {
      resolveReviewThread(input: { threadId: $id }) {
        thread { id isResolved }
      }
    }' --jq '"resolved: \(.data.resolveReviewThread.thread.id) (isResolved=\(.data.resolveReviewThread.thread.isResolved))"'
