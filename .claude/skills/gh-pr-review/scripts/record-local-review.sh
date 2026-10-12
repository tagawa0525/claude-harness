#!/usr/bin/env bash
# gh-pr-review: ローカルの /code-review の結果を、PR の head の記録としてコメントに残すスクリプト
#
# Usage: record-local-review.sh <PR の URL> <high> <medium> <low>
#   high / medium / low は、ローカルレビューで見つけた指摘の数（重大度は Copilot に揃える。ADR-0034）
#
# Copilot に再レビューを依頼する前と、依頼せずに終える前に、push した head をローカルで確かめた
# ことを残す。decide-next.sh、request-rereview.sh、pre-merge-check は、head の SHA の記録が
# あるかを見る。件数は、ローカルで見つけた数と後に Copilot が出した数を比べるために残す。
# 記録は先頭の 1 行（<!-- local-review sha=<SHA> high=<n> medium=<n> low=<n> -->）が本体で、
# 形を変えるときは scripts/lib/local-review.sh と pre_merge_check.rs も合わせる

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/pr-url.sh"
source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/local-review.sh"

if (($# != 4)) || ! parse_pr_url "$1"; then
  echo "Usage: $0 <PR の URL> <high> <medium> <low>" >&2
  exit 1
fi
for n in "$2" "$3" "$4"; do
  if [[ ! "$n" =~ ^[0-9]+$ ]]; then
    echo "Usage: $0 <PR の URL> <high> <medium> <low>（件数は 0 以上の整数: '$n'）" >&2
    exit 1
  fi
done

if ! head_sha=$(gh pr view "$PR_NUMBER" --json headRefOid -q .headRefOid) || [[ -z "$head_sha" ]]; then
  echo "ERROR: PR #${PR_NUMBER} の head を取得できませんでした（記録を付けていません）" >&2
  exit 1
fi

gh pr comment "$PR_NUMBER" --body "${LOCAL_REVIEW_MARK}sha=${head_sha} high=$2 medium=$3 low=$4 -->
ローカルの /code-review で ${head_sha:0:7} を確かめた: High $2 / Medium $3 / Low $4"
echo "RECORDED: ${head_sha}"
