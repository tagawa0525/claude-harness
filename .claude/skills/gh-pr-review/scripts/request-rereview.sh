#!/usr/bin/env bash
# gh-pr-review: Copilot に再レビューを依頼し、その応答を待つスクリプト
#
# Usage: request-rereview.sh <PR の URL>
#
# Copilot のレビューを受けたいときだけ使う（任意。head のローカルレビューの記録があればマージできる）。
#
# 1. head にローカルレビューの記録（record-local-review.sh。最後の記録から rebase だけした head を含む）があることを
#    確かめる。無ければ要求しない。
#    Copilot に出す前に、直した差分をローカルの /code-review で確かめる
# 2. requested_reviewers API で copilot-pull-request-reviewer にレビューを要求する
# 3. 要求が review_requested イベントとして登録されたことを確認し、その created_at
#    （GitHub 側の時刻）を基準に gh-wait-review.sh で待機する
#
# @copilot メンションでは再レビューは走らない（応答する copilot-swe-agent はコメントを
# 返すだけ）。要求に失敗してもメンション等にフォールバックしない。フォールバックすると
# レビューが来ていないのに依頼できたように見えるため、止めてユーザーに判断させる。
#
# 約10分待つため、Bash ツールでは run_in_background=true で実行すること。
# 終了コードは gh-wait-review.sh に従う（0=レビュー到着, 1=タイムアウト）。
# 要求そのものに失敗した場合は 1 で、待機には入らない。

set -euo pipefail

REVIEWER='copilot-pull-request-reviewer[bot]'

# 共有スクリプトは自身の位置から相対で解決する（<root>/skills/gh-pr-review/scripts → <root>/scripts）
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "${SCRIPT_DIR}/../../../scripts/lib/pr-url.sh"
source "${SCRIPT_DIR}/../../../scripts/lib/local-review.sh"

PR_URL="${1:-}"
if (($# != 1)) || ! parse_pr_url "$PR_URL"; then
  echo "Usage: $0 <PR の URL>" >&2
  exit 1
fi
REQUESTS_SCRIPT="${SCRIPT_DIR}/../../../scripts/gh-review-requests.sh"

# Copilot 宛てレビュー要求のうち最新の created_at（無ければ空）。
# 取得に失敗したら空を返さず止める。空扱いで続けると、要求が登録されているのに
# 「登録されませんでした」と誤診する
latest_request_at() {
  local events
  if ! events=$("$REQUESTS_SCRIPT" "$PR_URL_CANONICAL"); then
    echo "ERROR: PR #${PR_NUMBER} のレビュー要求を取得できませんでした（登録を確認できません）" >&2
    return 1
  fi
  tail -n 1 <<<"$events"
}

before=$(latest_request_at)

if ! head_sha=$(gh pr view "$PR_NUMBER" --json headRefOid -q .headRefOid) || [[ -z "$head_sha" ]]; then
  echo "ERROR: PR #${PR_NUMBER} の head を取得できませんでした（ローカルレビューの記録を確かめられません）" >&2
  exit 1
fi
if ! local_reviewed=$(local_review_covers "$head_sha"); then
  echo "ERROR: PR #${PR_NUMBER} のローカルレビューの記録を確かめられませんでした" >&2
  exit 1
fi
if [[ "$local_reviewed" != yes ]]; then
  echo "ERROR: head（${head_sha:0:7}）に High の残らないローカルレビューの記録がありません。Copilot に出す前に、直した差分をローカルの /code-review で確かめてください" >&2
  echo "次の一手: /gh-pr-review の「ローカルレビュー」の手順で /code-review を回したら ${SCRIPT_DIR}/record-local-review.sh ${PR_URL_CANONICAL} <SHA> <high> <medium> <low> で記録し（High が残れば直して回し直す）、もう一度 ${SCRIPT_DIR}/request-rereview.sh ${PR_URL_CANONICAL} で要求してください" >&2
  exit 1
fi

if ! err=$(gh api -X POST "repos/{owner}/{repo}/pulls/${PR_NUMBER}/requested_reviewers" \
             -f "reviewers[]=${REVIEWER}" 2>&1 >/dev/null); then
  echo "$err" >&2
  echo "ERROR: PR #${PR_NUMBER} に ${REVIEWER} のレビューを要求できませんでした" >&2
  echo "HINT: fork からの upstream PR（push 権限なし）では 404 になります。その場合は再レビューを依頼できないので、対応内容を PR コメントで伝えてレビュアーの判断を待ってください" >&2
  exit 1
fi

after=$(latest_request_at)
if [[ -z "$after" || "$after" == "$before" ]]; then
  echo "ERROR: レビュー要求が登録されませんでした（review_requested イベントが増えていません）" >&2
  echo "HINT: 前回の要求が保留のまま（レビューがまだ来ていない）か、リポジトリで Copilot code review が無効です。gh pr view ${PR_URL_CANONICAL} --json reviewRequests と /gh-actions-check ${PR_URL_CANONICAL} で確認してください" >&2
  exit 1
fi

echo "REQUESTED: ${REVIEWER}"
echo "SINCE: ${after}"

# リポジトリの .claude/ とデプロイ先の ~/.claude/ は同じ構造なので、どちらから実行しても
# 同じ版の gh-wait-review.sh が使われる（$HOME 固定だと未同期の旧版を呼びうる）
WAIT_SCRIPT="${SCRIPT_DIR}/../../../scripts/gh-wait-review.sh"
if [[ ! -x "$WAIT_SCRIPT" ]]; then
  echo "ERROR: gh-wait-review.sh が見つかりません: ${WAIT_SCRIPT}" >&2
  exit 1
fi
exec "$WAIT_SCRIPT" "$PR_URL_CANONICAL" --since "$after"
