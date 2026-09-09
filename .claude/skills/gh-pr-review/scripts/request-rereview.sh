#!/usr/bin/env bash
# gh-pr-review: Copilot に再レビューを依頼し、その応答を待つスクリプト
#
# Usage: request-rereview.sh <pr_number>
#
# 1. requested_reviewers API で copilot-pull-request-reviewer にレビューを要求する
# 2. 要求が review_requested イベントとして登録されたことを確認し、その created_at
#    （GitHub 側の時刻）を基準に gh-wait-review.sh で待機する
#
# 「@copilot 再レビューをお願いします」というメンションコメントでは再レビューは
# 走らない。メンションに応答するのは copilot-swe-agent（コーディングエージェント）で、
# 「対応を確認しました」等のコメントを返すだけで copilot-pull-request-reviewer の
# レビューは提出されない。依頼をメンションに切り替えた 2026-08-23 以降、このリポジトリの
# 全 PR で 2 周目のレビューがゼロになっていた（それ以前は最大 8 周回っていた）。
#
# 要求に失敗したときはメンションなどにフォールバックしない。フォールバックすると
# 「依頼したつもりでレビューが来ていない」状態が成功に見えてしまい、今回のように
# 気づかれないまま放置される。止めてユーザーに判断させる。
#
# 約10分待つため、Bash ツールでは run_in_background=true で実行すること。
# 終了コードは gh-wait-review.sh に従う（0=レビュー到着, 1=タイムアウト）。
# 要求そのものに失敗した場合は 1 で、待機には入らない。

set -euo pipefail

REVIEWER='copilot-pull-request-reviewer[bot]'

PR_NUMBER="${1:-}"
if [[ -z "$PR_NUMBER" || $# -gt 1 ]]; then
  echo "Usage: $0 <pr_number>" >&2
  exit 1
fi
if ! [[ "$PR_NUMBER" =~ ^[0-9]+$ ]]; then
  echo "ERROR: pr_number は数値で指定してください: '${PR_NUMBER}'" >&2
  exit 1
fi

REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# 共有スクリプトは自身の位置から相対で解決する（<root>/skills/gh-pr-review/scripts → <root>/scripts）
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REQUESTS_SCRIPT="${SCRIPT_DIR}/../../../scripts/gh-review-requests.sh"

# Copilot 宛てレビュー要求のうち最新の created_at（無ければ空）。
# 取得に失敗したら空を返さず止める。空扱いで続けると、要求が登録されているのに
# 「登録されませんでした」と誤診する
latest_request_at() {
  local events
  if ! events=$("$REQUESTS_SCRIPT" "$PR_NUMBER"); then
    echo "ERROR: PR #${PR_NUMBER} のレビュー要求を取得できませんでした（登録を確認できません）" >&2
    return 1
  fi
  tail -n 1 <<<"$events"
}

before=$(latest_request_at)

if ! err=$(gh api -X POST "repos/${REPO}/pulls/${PR_NUMBER}/requested_reviewers" \
             -f "reviewers[]=${REVIEWER}" 2>&1 >/dev/null); then
  echo "$err" >&2
  echo "ERROR: PR #${PR_NUMBER} に ${REVIEWER} のレビューを要求できませんでした" >&2
  echo "HINT: fork からの upstream PR（push 権限なし）では 404 になります。その場合は再レビューを依頼できないので、対応内容を PR コメントで伝えてレビュアーの判断を待ってください" >&2
  exit 1
fi

after=$(latest_request_at)
if [[ -z "$after" || "$after" == "$before" ]]; then
  echo "ERROR: レビュー要求が登録されませんでした（review_requested イベントが増えていません）" >&2
  echo "HINT: 前回の要求が保留のまま（レビューがまだ来ていない）か、リポジトリで Copilot code review が無効です。gh pr view ${PR_NUMBER} --json reviewRequests と /gh-actions-check ${PR_NUMBER} で確認してください" >&2
  exit 1
fi

echo "REQUESTED: ${REVIEWER}"
echo "SINCE: ${after}"

# リポジトリの .claude/ と配備先の ~/.claude/ は同じ構造なので、どちらから実行しても
# 同じ版の gh-wait-review.sh が使われる（$HOME 固定だと未同期の旧版を呼びうる）
WAIT_SCRIPT="${SCRIPT_DIR}/../../../scripts/gh-wait-review.sh"
if [[ ! -x "$WAIT_SCRIPT" ]]; then
  echo "ERROR: gh-wait-review.sh が見つかりません: ${WAIT_SCRIPT}" >&2
  exit 1
fi
exec "$WAIT_SCRIPT" "$PR_NUMBER" --since "$after"
