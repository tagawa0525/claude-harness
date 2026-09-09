#!/usr/bin/env bash
# gh-pr-review: レビュー対応後に「次に取るべき行動」を判定するスクリプト
#
# Usage: decide-next.sh <pr_number> [--max-rounds N]   (デフォルト 5)
#
# 出力（テキスト）:
#   ROUND: <n>          周回数。Copilot へのレビュー要求の件数（PR 作成時の自動要求を
#                       含む）と Copilot レビュー件数の多い方。要求イベントを伴わずに
#                       レビューが付くリポジトリでも周回を見失わないため
#   MAX_ROUNDS: <n>
#   RESPONSE: review | none
#                       直近のレビュー要求より後に提出された Copilot レビューの有無
#                       （要求イベントが無ければ最初のレビューを対象にする）
#   INLINE_COMMENTS / SUPPRESSED_COMMENTS （RESPONSE=review のとき）
#   VERDICT: <判定>
#     REREVIEW             インライン指摘あり・上限未満 → 対応して再レビューを依頼する
#     STOP_LIMIT           インライン指摘あり・上限到達 → 対応するが再レビューは依頼せず
#                          残指摘を完了報告に列挙してユーザー判断に委ねる
#     STOP_SUPPRESSED_ONLY Suppressed comments のみ → 対応するが再レビューは依頼しない
#     STOP_CLEAN           指摘なし → マージへ
#     REVIEW_FAILED        Copilot がレビューできずに終わった → 原因を調べて依頼し直す
#                          （指摘ゼロと同じ見た目になるため別扱いにする）
#     WAITING              要求後のレビューがまだ無い → gh-wait-review.sh で待つ
#   --- suppressed --- 以降に本文
#
# 周回数はレビュー要求（review_requested）の件数で数える。Copilot のコメントは
# 数えない。@copilot メンションに応答するのは copilot-swe-agent（コーディング
# エージェント）で、「対応を確認しました」等のコメントを返してもレビューは
# 提出されない。これをレビュー応答として扱っていたため、指摘が残っていても
# 1 周で終わっていた（2026-08-23〜09-08）。

set -euo pipefail

PR_NUMBER="${1:-}"
MAX_ROUNDS=5
if [[ "${2:-}" == "--max-rounds" ]]; then
  MAX_ROUNDS="${3:?--max-rounds には数値が必要です}"
fi

if [[ -z "$PR_NUMBER" ]]; then
  echo "Usage: $0 <pr_number> [--max-rounds N]" >&2
  exit 1
fi

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner')

# Copilot 宛てレビュー要求の created_at を発生順に取得。
# 取得に失敗したら空扱いで続けない。周回数を過少に見積もり、上限判定がずれる
if ! requests=$("${SCRIPT_DIR}/../../../scripts/gh-review-requests.sh" "$PR_NUMBER"); then
  echo "ERROR: PR #${PR_NUMBER} のレビュー要求を取得できませんでした（周回数が数えられません）" >&2
  exit 1
fi
request_count=$(grep -c . <<<"$requests" || true)
last_request_at=$(tail -n 1 <<<"$requests")

# 最新の Copilot レビュー
latest=$("${SCRIPT_DIR}/get-latest-review.sh" "$PR_NUMBER")
review_count=$(sed -n 's/^ROUND: //p' <<<"$latest")
last_review_at=""
if (( review_count > 0 )); then
  review_id=$(sed -n 's/^REVIEW_ID: //p' <<<"$latest")
  last_review_at=$(gh api "repos/${REPO}/pulls/${PR_NUMBER}/reviews/${review_id}" --jq '.submitted_at')
fi

# 要求イベントを取得できないリポジトリでも周回を見失わないよう、多い方を周回数とする
round=$(( request_count > review_count ? request_count : review_count ))

# 直近の要求より後に提出されたレビューだけを今周の応答とみなす
# （ISO 8601 は文字列比較で時系列順。要求が無ければ最初のレビューが対象）
if [[ -n "$last_review_at" && "$last_review_at" > "$last_request_at" ]]; then
  response=review
else
  response=none
fi

echo "ROUND: ${round}"
echo "MAX_ROUNDS: ${MAX_ROUNDS}"
echo "RESPONSE: ${response}"

if [[ "$response" == "none" ]]; then
  echo "VERDICT: WAITING"
  exit 0
fi

failed=$(sed -n 's/^REVIEW_FAILED: //p' <<<"$latest")
inline=$(sed -n 's/^INLINE_COMMENTS: //p' <<<"$latest")
suppressed=$(sed -n 's/^SUPPRESSED_COMMENTS: //p' <<<"$latest")
echo "REVIEW_ID: ${review_id}"
echo "HEADLINE: $(sed -n 's/^HEADLINE: //p' <<<"$latest")"
echo "INLINE_COMMENTS: ${inline}"
echo "SUPPRESSED_COMMENTS: ${suppressed}"
if [[ "$failed" == "yes" ]]; then
  echo "VERDICT: REVIEW_FAILED"
elif (( inline > 0 )); then
  if (( round < MAX_ROUNDS )); then
    echo "VERDICT: REREVIEW"
  else
    echo "VERDICT: STOP_LIMIT"
  fi
elif (( suppressed > 0 )); then
  echo "VERDICT: STOP_SUPPRESSED_ONLY"
else
  echo "VERDICT: STOP_CLEAN"
fi
if (( suppressed > 0 )); then
  sed -n '/^--- suppressed ---$/,$p' <<<"$latest"
fi
