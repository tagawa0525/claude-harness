#!/usr/bin/env bash
# gh-pr-review: レビューの周回を 0 から数え直す印のコメントを PR に付ける
#
# Usage: reset-rounds.sh <pr_number> <理由>
#
# ユーザーの追加の要望に対応した変更は、それまでの指摘への対応とは別の仕事なので、
# 新しい周回として数える。decide-next.sh は最後の印より後のレビュー要求だけを数える。
# 理由は PR に残り、いつ誰の要望で数え直したかを後から追える。

set -euo pipefail

if (($# != 2)) || [[ -z "$2" ]]; then
  echo "Usage: $0 <pr_number> <理由>" >&2
  exit 1
fi

gh pr comment "$1" --body "<!-- review-rounds-reset -->
レビューの周回を数え直す: $2"
