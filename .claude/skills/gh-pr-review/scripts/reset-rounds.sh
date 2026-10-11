#!/usr/bin/env bash
# gh-pr-review: レビューの周回を 0 から数え直す印のコメントを PR に付ける
#
# Usage: reset-rounds.sh <PR の URL> <理由>
#
# ユーザーの追加の要望に対応した変更は、それまでの指摘への対応とは別の仕事なので、
# 新しい周回として数える。decide-next.sh は最後の印より後のレビュー要求だけを数える。
# 理由は PR に残り、いつ誰の要望で数え直したかを後から追える。

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/pr-url.sh"
if (($# != 2)) || ! parse_pr_url "$1" || [[ -z "$2" ]]; then
  echo "Usage: $0 <PR の URL> <理由>" >&2
  exit 1
fi

gh pr comment "$PR_NUMBER" --body "<!-- review-rounds-reset -->
レビューの周回を数え直す: $2"
