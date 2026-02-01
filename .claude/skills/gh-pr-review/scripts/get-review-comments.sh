#!/usr/bin/env bash
# gh-pr-review: レビューコメント取得スクリプト
#
# Usage: get-review-comments.sh <pr_number> [--unresolved]
#
# Output: JSON形式のレビューコメント一覧

set -euo pipefail

PR_NUMBER="${1:-}"
UNRESOLVED_ONLY="${2:-}"

if [[ -z "$PR_NUMBER" ]]; then
  echo "Usage: $0 <pr_number> [--unresolved]" >&2
  exit 1
fi

# リポジトリ情報を取得
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# gh pr-review 拡張が利用可能かチェック
if gh extension list | grep -q 'pr-review'; then
  # gh pr-review 拡張を使用（推奨）
  if [[ "$UNRESOLVED_ONLY" == "--unresolved" ]]; then
    gh pr-review review view "$PR_NUMBER" -R "$REPO" \
      --unresolved --include-comment-node-id --json 2>/dev/null || {
      echo "gh pr-review failed, falling back to gh api" >&2
      USE_API=1
    }
  else
    gh pr-review review view "$PR_NUMBER" -R "$REPO" \
      --include-comment-node-id --json 2>/dev/null || {
      echo "gh pr-review failed, falling back to gh api" >&2
      USE_API=1
    }
  fi
fi

# gh api フォールバック
if [[ "${USE_API:-}" == "1" ]] || ! gh extension list | grep -q 'pr-review'; then
  # レビューコメント（インラインコメント）を取得
  gh api "repos/${REPO}/pulls/${PR_NUMBER}/comments" \
    --jq '[.[] | {
      id: .id,
      node_id: .node_id,
      path: .path,
      line: .line,
      body: .body,
      user: .user.login,
      created_at: .created_at,
      in_reply_to_id: .in_reply_to_id,
      html_url: .html_url
    }]'
fi
