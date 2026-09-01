#!/usr/bin/env bash
# gh-pr-review: レビュー対応後に「次に取るべき行動」を判定するスクリプト
#
# Usage: decide-next.sh <pr_number> [--max-rounds N]   (デフォルト 5)
#
# 出力（テキスト）:
#   ROUND: <n>          1 + これまでに送った再レビュー依頼（@copilot メンション）の回数
#   MAX_ROUNDS: <n>
#   RESPONSE: review | comment | none
#                       直近の再レビュー依頼より後に届いた Copilot の応答種別
#                       （依頼がまだ無ければ最初のレビューを対象にする）
#   INLINE_COMMENTS / SUPPRESSED_COMMENTS （RESPONSE=review のとき）
#   VERDICT: <判定>
#     REREVIEW             インライン指摘あり・上限未満 → 対応して再レビューを依頼する
#     STOP_LIMIT           インライン指摘あり・上限到達 → 対応するが再レビューは依頼せず
#                          残指摘を完了報告に列挙してユーザー判断に委ねる
#     STOP_SUPPRESSED_ONLY Suppressed comments のみ → 対応するが再レビューは依頼しない
#     STOP_CLEAN           指摘なし → マージへ
#     COMMENT_ONLY         Copilot がコメントだけで応答 → 本文を読んで判定する
#                          （対応確認のみなら STOP_CLEAN 相当、新しい指摘を含むなら
#                            ROUND を見て REREVIEW / STOP_LIMIT 相当として扱う）
#     WAITING              依頼後の応答がまだ無い → gh-wait-review.sh で待つ
#   --- suppressed --- / --- comment ---  以降に本文
#
# 周回数は再レビュー依頼の回数から算出する。Copilot が正式なレビューを提出せず
# コメントだけで応答した周は Copilot レビュー件数が増えないため、レビュー件数を
# 周回数に使うと上限判定がずれる。

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

# PR コメント（issue comment）を作成順に取得
comments=$(gh api --paginate "repos/${REPO}/issues/${PR_NUMBER}/comments?per_page=100" \
  --jq '.[] | {created_at, login: .user.login, type: .user.type, body}' | jq -s '.')

# 再レビュー依頼 = Bot 以外が投稿した @copilot メンション
requests=$(jq '[.[] | select(.type != "Bot" and (.body | test("@copilot"; "i")))]' <<<"$comments")
request_count=$(jq 'length' <<<"$requests")
last_request_at=$(jq -r 'if length > 0 then .[-1].created_at else "" end' <<<"$requests")
round=$(( request_count + 1 ))

# Copilot の応答コメント（gh-wait-review.sh と同じ許可リスト）
copilot_comments=$(jq '[.[] | select(.type == "Bot"
  and ((.login | ascii_downcase)
       | IN("copilot", "copilot-swe-agent[bot]", "copilot-pull-request-reviewer[bot]")))]' <<<"$comments")
last_comment_at=$(jq -r 'if length > 0 then .[-1].created_at else "" end' <<<"$copilot_comments")

# 最新の Copilot レビュー
latest=$("${SCRIPT_DIR}/get-latest-review.sh" "$PR_NUMBER")
review_count=$(sed -n 's/^ROUND: //p' <<<"$latest")
last_review_at=""
if (( review_count > 0 )); then
  review_id=$(sed -n 's/^REVIEW_ID: //p' <<<"$latest")
  last_review_at=$(gh api "repos/${REPO}/pulls/${PR_NUMBER}/reviews/${review_id}" --jq '.submitted_at')
fi

# 直近の依頼より後に届いた応答を採用する（ISO 8601 は文字列比較で時系列順）。
# レビュー提出があれば、その後にフォローコメントが付いていても常にレビューを優先する
# （コメントを優先するとインライン指摘のあるレビューを COMMENT_ONLY で見落とす）
review_is_new=$([[ -n "$last_review_at" && "$last_review_at" > "$last_request_at" ]] && echo 1 || echo 0)
comment_is_new=$([[ -n "$last_comment_at" && "$last_comment_at" > "$last_request_at" ]] && echo 1 || echo 0)

if (( review_is_new )); then
  response=review
elif (( comment_is_new )); then
  response=comment
else
  response=none
fi

echo "ROUND: ${round}"
echo "MAX_ROUNDS: ${MAX_ROUNDS}"
echo "RESPONSE: ${response}"

case "$response" in
  review)
    inline=$(sed -n 's/^INLINE_COMMENTS: //p' <<<"$latest")
    suppressed=$(sed -n 's/^SUPPRESSED_COMMENTS: //p' <<<"$latest")
    echo "REVIEW_ID: ${review_id}"
    echo "HEADLINE: $(sed -n 's/^HEADLINE: //p' <<<"$latest")"
    echo "INLINE_COMMENTS: ${inline}"
    echo "SUPPRESSED_COMMENTS: ${suppressed}"
    if (( inline > 0 )); then
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
    ;;
  comment)
    echo "VERDICT: COMMENT_ONLY"
    echo "--- comment ---"
    jq -r '.[-1] | "\(.login) (\(.created_at)):\n\(.body)"' <<<"$copilot_comments"
    ;;
  none)
    echo "VERDICT: WAITING"
    ;;
esac
