#!/bin/sh
# gh-pr-review: 最新の Copilot レビューの要約を取得するスクリプト。中身は claude-hooks script get-latest-review（Rust に
# して Windows でも jq なしで動かす。出力の各行の意味もそこに書く）。スキルの文と decide-next.sh はこのスクリプトの
# 名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: get-latest-review.sh <PR の URL>
#
# 出力（テキスト）: ROUND、REVIEW_ID、STATE、HEADLINE、REVIEW_FAILED、SUCCESSFUL_REVIEWS、INLINE_COMMENTS、
#   SUPPRESSED_COMMENTS、SEVERITY、REMAINING_HIGH|MEDIUM|LOW、PREVIOUSLY_MISSED、--- suppressed ---、
#   --- previously missed ---
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script get-latest-review "$@"
