#!/bin/sh
# gh-pr-review: レビュー対応後に「次に取るべき行動」を判定するスクリプト。中身は claude-hooks script decide-next
# （Rust にして Windows でも jq なしで動かす。出力の各行と VERDICT の意味もそこに書く）。スキルの文はこのスクリプトの
# 名前とパスで呼ぶので、ファイルは残して呼ぶ
#
# Usage: decide-next.sh <PR の URL>
#
# 出力（テキスト）: ROUND、RESPONSE、HEAD、REVIEWED、HEAD_REVIEWED、UNRESOLVED、LOCAL_REVIEWED、（RESPONSE=review のとき）
#   REVIEW_ID、HEADLINE、INLINE_COMMENTS、SUPPRESSED_COMMENTS、SEVERITY、REMAINING_HIGH、PREVIOUSLY_MISSED、VERDICT、
#   --- suppressed --- / --- previously missed --- 以降に本文
. "$(dirname "$0")/../../../scripts/lib/claude-hooks-script.sh"
claude_hooks_script decide-next "$@"
