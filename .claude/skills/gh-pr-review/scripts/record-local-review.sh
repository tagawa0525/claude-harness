#!/usr/bin/env bash
# gh-pr-review: ローカルの /code-review の結果を、PR の head の記録としてコメントに残すスクリプト
#
# Usage: record-local-review.sh <PR の URL> <SHA> <high> <medium> <low>
#   SHA はローカルでレビューしたコミット（git rev-parse HEAD）。PR の head と一致しなければ記録しない
#   （レビューの後に head が動いたなら、その head はレビューしていない）
#   high / medium / low は、ローカルレビューで見つけた指摘の数（重大度は Copilot に揃える。ADR-0034）
#
# Copilot に再レビューを依頼する前と、依頼せずに終える前に、push した head をローカルで確かめた
# ことを残す。decide-next.sh、request-rereview.sh、pre-merge-check は、head の SHA の記録が
# あるかを見る。件数は、その head で検証して正しいと判断した指摘の数で、後に Copilot が出した数と比べるために残す。
# 記録は先頭の 1 行（<!-- local-review sha=<SHA> high=<n> medium=<n> low=<n> -->）が本体で、
# 形を変えるときは scripts/lib/local-review.sh と pre_merge_check.rs も合わせる

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/pr-url.sh"
source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib/local-review.sh"

USAGE="Usage: $0 <PR の URL> <レビューした SHA> <high> <medium> <low>"
if (($# != 5)) || ! parse_pr_url "$1"; then
  echo "$USAGE" >&2
  exit 1
fi
reviewed="$2"
if [[ ! "$reviewed" =~ ^[0-9a-f]{7,40}$ ]]; then
  echo "${USAGE}（SHA は 7〜40 桁の 16 進: '$reviewed'）" >&2
  exit 1
fi
shift 2
for n in "$@"; do
  # 読む側は high=0 の字面で比べるので、先頭の 0 を許さない
  if [[ ! "$n" =~ ^(0|[1-9][0-9]*)$ ]]; then
    echo "${USAGE}（件数は 0 以上の整数で、先頭に 0 を付けない: '$n'）" >&2
    exit 1
  fi
done

if ! head_sha=$(gh pr view "$PR_NUMBER" --json headRefOid -q .headRefOid) || [[ -z "$head_sha" ]]; then
  echo "ERROR: PR #${PR_NUMBER} の head を取得できませんでした（記録を付けていません）" >&2
  exit 1
fi
# 記録を数えるのは書き込める人のコメントだけなので、push できない（上流の PR）なら付けても効かない
if ! can_push=$(gh api "repos/{owner}/{repo}" --jq .permissions.push); then
  echo "ERROR: ${GH_REPO} の権限を取得できませんでした（記録を付けていません）" >&2
  exit 1
fi
if [[ "$can_push" != true ]]; then
  echo "ERROR: ${GH_REPO} に push できないので、記録を付けても数えられません（記録を付けていません）" >&2
  exit 1
fi
if [[ "$head_sha" != "$reviewed"* ]]; then
  echo "ERROR: レビューした ${reviewed:0:7} は PR #${PR_NUMBER} の head（${head_sha:0:7}）ではありません。head を push してからレビューし直してください（記録を付けていません）" >&2
  exit 1
fi

gh pr comment "$PR_NUMBER" --body "${LOCAL_REVIEW_MARK}sha=${head_sha} high=$1 medium=$2 low=$3 -->
ローカルの /code-review で ${head_sha:0:7} を確かめた: High $1 / Medium $2 / Low $3"
echo "RECORDED: ${head_sha}"
if (($1 > 0)); then
  echo "NOTE: High の残る記録は、レビュー済みとして数えない。直して push し、回し直して記録する"
fi
