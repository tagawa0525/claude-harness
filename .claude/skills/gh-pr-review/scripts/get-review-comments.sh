#!/usr/bin/env bash
# gh-pr-review: レビューコメント取得スクリプト
#
# Usage: get-review-comments.sh <pr_number> [--unresolved]
#
# Output: JSON 配列。1 要素 = 1 コメント（スレッド内の返信も含む、作成順）
#   id, node_id, path, line, body, user, created_at, in_reply_to_id, html_url,
#   thread_id（resolve-thread.sh の対象）, is_resolved, is_outdated
#
# REST の /pulls/{n}/comments はスレッドの resolve 状態を持たないため、GraphQL の
# reviewThreads から取得する。--unresolved は isResolved == false のスレッドに限定する。
# 以前は gh pr-review 拡張を優先していたが、拡張のフラグ変更で常に REST へ
# フォールバックし --unresolved が効かない状態になっていたため、拡張依存をやめた。

set -euo pipefail

PR_NUMBER="${1:-}"
UNRESOLVED_ONLY="${2:-}"

if [[ -z "$PR_NUMBER" ]]; then
  echo "Usage: $0 <pr_number> [--unresolved]" >&2
  exit 1
fi
if ! [[ "$PR_NUMBER" =~ ^[0-9]+$ ]]; then
  echo "ERROR: pr_number は数値で指定してください: '${PR_NUMBER}'" >&2
  exit 1
fi
if [[ -n "$UNRESOLVED_ONLY" && "$UNRESOLVED_ONLY" != "--unresolved" ]]; then
  echo "ERROR: 不明なオプション: ${UNRESOLVED_ONLY}" >&2
  exit 1
fi

OWNER=$(gh repo view --json owner -q '.owner.login')
NAME=$(gh repo view --json name -q '.name')

# スレッドは --paginate で全件。スレッド内コメントは 100 件まで（通常十分。超える場合は
# 末尾を取りこぼすので、hasNextPage を見て警告する）
raw=$(gh api graphql --paginate \
  -F owner="$OWNER" -F name="$NAME" -F number="$PR_NUMBER" \
  -f query='
    query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $number) {
          reviewThreads(first: 100, after: $endCursor) {
            pageInfo { hasNextPage endCursor }
            nodes {
              id
              isResolved
              isOutdated
              path
              line
              comments(first: 100) {
                pageInfo { hasNextPage }
                nodes {
                  databaseId
                  id
                  body
                  author { login }
                  createdAt
                  url
                  replyTo { databaseId }
                }
              }
            }
          }
        }
      }
    }' --jq '.data.repository.pullRequest.reviewThreads.nodes[]')

if jq -e 'select(.comments.pageInfo.hasNextPage)' <<<"$raw" >/dev/null 2>&1; then
  echo "WARN: 100 件を超える返信を持つスレッドがあり、末尾の返信は省略されています" >&2
fi

unresolved=false
[[ "$UNRESOLVED_ONLY" == "--unresolved" ]] && unresolved=true

jq -s --argjson unresolved "$unresolved" '
  [ .[]
    | select(($unresolved | not) or (.isResolved | not))
    | . as $t
    | .comments.nodes[]
    | {
        id: .databaseId,
        node_id: .id,
        path: $t.path,
        line: $t.line,
        body: .body,
        user: (.author.login // null),
        created_at: .createdAt,
        in_reply_to_id: (.replyTo.databaseId // null),
        html_url: .url,
        thread_id: $t.id,
        is_resolved: $t.isResolved,
        is_outdated: $t.isOutdated
      }
  ] | sort_by(.created_at)
' <<<"$raw"
