#!/usr/bin/env bash
# gh-pr-review: レビュースレッドを resolve するスクリプト
#
# Usage: resolve-thread.sh <pr_number> <comment_id> [--allow-human]
#
#   comment_id: スレッド内の任意のレビューコメントの数値 ID
#               （get-review-comments.sh の id / URL の #discussion_r{id}）
#
# REST には resolve API がないため GraphQL resolveReviewThread を使う。
# 対応済みスレッドを返信後に resolve することで、bot の再レビュー自動化の
# トリガーになるリポジトリでも整合が取れる。
#
# スレッドを閉じるのはレビュアーの権限。bot（Copilot 等）が起こしたスレッドは対応後に
# resolve するが、人間が起こしたスレッドは返信だけにして相手に委ねる。人間のスレッドは
# 既定で拒否（exit 1）し、ユーザーが明示的に求めたときだけ --allow-human で resolve する。
# 生の GraphQL でこの判定を迂回しないよう、hooks/guard-gh-api.sh が resolveReviewThread を deny する

set -euo pipefail

PR_NUMBER="${1:-}"
COMMENT_ID="${2:-}"
ALLOW_HUMAN=0
if [[ "${3:-}" == "--allow-human" ]]; then
  ALLOW_HUMAN=1
elif [[ -n "${3:-}" ]]; then
  echo "ERROR: 不明なオプション: ${3}" >&2
  exit 1
fi

if [[ -z "$PR_NUMBER" ]] || [[ -z "$COMMENT_ID" ]]; then
  echo "Usage: $0 <pr_number> <comment_id> [--allow-human]" >&2
  exit 1
fi

# 数値以外（URL 断片の discussion_r123 など）は jq --argjson でパースエラーになるため先に弾く
if ! [[ "$PR_NUMBER" =~ ^[0-9]+$ ]]; then
  echo "ERROR: pr_number は数値で指定してください: '${PR_NUMBER}'" >&2
  exit 1
fi
if ! [[ "$COMMENT_ID" =~ ^[0-9]+$ ]]; then
  echo "ERROR: comment_id は数値で指定してください（URL の #discussion_r の後ろの数字）: '${COMMENT_ID}'" >&2
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
              comments(first: 100) { nodes { databaseId author { __typename login } } }
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

# スレッドを起こした人（先頭コメントの投稿者）が bot でなければ既定で拒否する
starter_type=$(jq -r '.comments.nodes[0].author.__typename // "unknown"' <<<"$thread")
starter_login=$(jq -r '.comments.nodes[0].author.login // "unknown"' <<<"$thread")
if [[ "$starter_type" != "Bot" ]] && (( ! ALLOW_HUMAN )); then
  echo "ERROR: スレッド ${thread_id} は人間（${starter_login}）が起こしたものです。返信だけ行い、resolve はレビュアーに委ねてください" >&2
  echo "       ユーザーから明示的に resolve を求められた場合のみ --allow-human を付けて再実行してください" >&2
  exit 1
fi

gh api graphql -F id="$thread_id" \
  -f query='
    mutation($id: ID!) {
      resolveReviewThread(input: { threadId: $id }) {
        thread { id isResolved }
      }
    }' --jq '"resolved: \(.data.resolveReviewThread.thread.id) (isResolved=\(.data.resolveReviewThread.thread.isResolved))"'
