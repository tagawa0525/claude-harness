#!/usr/bin/env bash
# gh-wait-review.sh — PRの自動レビュー到着を漸増バックオフで待つ
#
# 使い方: gh-wait-review.sh <PR の URL> [--since <ISO 8601 時刻>]
#   対象は PR の URL で受け、gh を URL のリポジトリに向ける（cwd のリポジトリに依らない。ADR-0032）
#   --since 省略時は「最後の Copilot へのレビュー要求」を基準にし、それ以降に提出された
#   レビューを待つ。要求が 1 件も無ければ、PR 作成直後の自動要求の記録を約 10 秒
#   待ったうえでエラーで終わる（要求していないレビューは来ないため）
#   --since 指定時はその時刻を基準にする（~/.claude/skills/gh-pr-review/scripts/
#   request-rereview.sh が要求の created_at を渡す。このディレクトリには無い）
#
# 基準を要求に紐づけるのは、起動時点の最新レビューを基準にすると来ないレビューを
# 10 分待ってしまうため。要求後のレビューが既にあれば即座に成功するので、続けて
# 呼んでも無害。基準は GitHub 側の時刻で持つ（ローカル時計のずれで取りこぼさないため）
#
# 検出するのは Copilot のレビュー提出（submittedAt）だけ。PR コメントは数えない
# （@copilot に応答する copilot-swe-agent はコメントを返すだけでレビューを提出しない）
#
# 待機間隔（秒）は GH_WAIT_INTERVALS、要求の記録待ちの間隔は GH_WAIT_REQUEST_INTERVALS
# で上書きできる（テスト用）
#
# 終了コード:
#   0 = レビュー到着
#   1 = タイムアウト（約10分）
#   3 = 対象PRが見つからない
#   4 = gh が利用できない（未インストール / 未認証）
#   5 = 引数エラー（不明なオプション / --since の値なし / PR の URL でない・無い・重複指定）
#   6 = レビュー要求がない（待っても来ない）、または要求を取得できない
#   7 = レビューの実行が失敗している（トークン切れ・内部エラー等。待っても来ない）
set -u

source "$(dirname "${BASH_SOURCE[0]}")/lib/pr-url.sh"

pr_url=""
since=""
while (( $# > 0 )); do
  case "$1" in
    --since)
      since="${2:-}"
      if [[ -z "$since" ]]; then
        echo "ERROR: --since には ISO 8601 の時刻が必要です"
        exit 5
      fi
      shift 2
      ;;
    -*)
      echo "ERROR: 不明なオプション: $1"
      exit 5
      ;;
    *)
      if [[ -n "$pr_url" ]]; then
        echo "ERROR: PR の URL は 1 つだけ指定してください: '$pr_url' と '$1'"
        exit 5
      fi
      pr_url="$1"
      shift
      ;;
  esac
done
if ! parse_pr_url "$pr_url"; then
  exit 5
fi

# PR解決やポーリングの失敗を「PRなし」と混同しないよう、ghの利用可否を先に検証する
if ! command -v gh >/dev/null 2>&1; then
  echo "ERROR: gh コマンドが見つかりません"
  exit 4
fi
if ! gh auth status >/dev/null 2>&1; then
  echo "ERROR: gh が未認証です。gh auth login を実行してください"
  exit 4
fi

# ポーリング中のカウント関数はghの失敗を0件扱いするため、
# PRの存在はここで一度だけ検証し「PRなし」がtimeoutに化けるのを防ぐ
if ! gh pr view "$PR_NUMBER" --json number >/dev/null 2>&1; then
  echo "ERROR: PR #${PR_NUMBER} が見つからないか、アクセスできません"
  exit 3
fi

# PR のレビュー一覧（JSON）。取得に失敗したら空の一覧として扱い、待機を続ける
reviews_json() {
  gh pr view "$PR_NUMBER" --json reviews 2>/dev/null || echo '{"reviews":[]}'
}

# 最新の Copilot レビューの提出時刻（ISO 8601、無ければ空）。
# 待っているのは要求した Copilot のレビューだけ。PR 作成者がインラインコメントに
# 返信すると作成者名義の COMMENTED レビューが作られるため、投稿者を問わずに見ると
# 返信を「到着」と誤判定する。reviews の author には種別が無いので、
# gh-review-requests.sh と同じく login に copilot を含むかで判定する。
# 投稿者が削除されたレビューは author が null になる。そのまま文字列処理に渡すと
# jq ごと失敗して Copilot のレビューまで捨てるため、空文字に寄せてから判定する
latest_review_at() {
  reviews_json | jq -r '[.reviews[]
                         | select((.author.login // "") | ascii_downcase | test("copilot"))
                         | .submittedAt] | max // ""' 2>/dev/null || echo ""
}

# head の Copilot チェックが失敗しているか（トークン切れ・内部エラーでレビューが
# 走らなかった場合。待っても来ないので、10分待たずに切り上げるために見る）。
# head は待機中の push で変わるので毎回取り直す（古い head の失敗で打ち切らない）。
# 取得できないときは「失敗していない」として待機を続ける（誤って打ち切らない）
copilot_run_failed() {
  local head_sha names
  head_sha=$(gh pr view "$PR_NUMBER" --json headRefOid -q '.headRefOid' 2>/dev/null) || return 1
  [[ -n "$head_sha" ]] || return 1
  # チェックの多い PR で取りこぼさないよう全ページ見る。--paginate に配列を返す --jq を
  # 渡すと不正な JSON になるため、名前をストリーム出力して行数で数える
  names=$(gh api --paginate "repos/{owner}/{repo}/commits/${head_sha}/check-runs?per_page=100" \
    --jq '.check_runs[]
          | select((.name | ascii_downcase | test("copilot"))
                   and .status == "completed"
                   and ((.conclusion // "") | IN("success", "neutral", "skipped") | not))
          | .name' 2>/dev/null) || return 1
  [[ -n "$names" ]]
}

report_reviews() {
  reviews_json | jq -r '.reviews[] | "- \(.author.login): \(.state)"'
}

# 基準時刻より後のレビュー提出をもって「待っていたレビュー」と判定する
# （ISO 8601 は文字列比較で時系列順）。
# --since 未指定なら最後の Copilot へのレビュー要求を基準にする
if [[ -z "$since" ]]; then
  requests_script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/gh-review-requests.sh"
  if [[ ! -x "$requests_script" ]]; then
    echo "ERROR: gh-review-requests.sh が見つかりません: ${requests_script}"
    exit 6
  fi
  # PR 作成時の自動要求は作成の 1〜5 秒後に記録される（PR #239〜#241 で実測）。
  # gh pr create の直後に呼ばれても取りこぼさないよう、要求が見つかるまで少し読み直す。
  # 間隔（秒）は GH_WAIT_REQUEST_INTERVALS で上書きできる（テスト用）
  read -ra request_intervals <<<"${GH_WAIT_REQUEST_INTERVALS:-2 3 5}"
  since=""
  for interval in 0 "${request_intervals[@]}"; do
    sleep "$interval"
    if ! requests=$("$requests_script" "$PR_URL_CANONICAL"); then
      echo "ERROR: PR #${PR_NUMBER} のレビュー要求を取得できません（timeline API が失敗）"
      exit 6
    fi
    since=$(tail -n 1 <<<"$requests")
    [[ -n "$since" ]] && break
  done
  if [[ -z "$since" ]]; then
    echo "ERROR: PR #${PR_NUMBER} には Copilot へのレビュー要求がありません。要求していないレビューは来ないので待ちません"
    # デプロイ先（~/.claude）の旧版を案内しないよう、自分と同じツリーのスクリプトを指す
    echo "次の一手: $(cd "$(dirname "$requests_script")/.." && pwd)/skills/gh-pr-review/scripts/request-rereview.sh ${PR_URL_CANONICAL} でレビューを要求してください（要求の前に、head のローカルレビューを record-local-review.sh で記録する。ADR-0034）"
    exit 6
  fi
  echo "INFO: 最後のレビュー要求は ${since}。それ以降のレビューの到着を待ちます"
else
  echo "INFO: ${since} より新しいレビューの到着を待ちます"
fi

# 新しいレビューが到着していれば報告して成功(0)、未着なら失敗(1)
check_new_review() {
  local ra
  ra=$(latest_review_at)
  if [[ -n "$ra" && "$ra" > "$since" ]]; then
    echo "OK: PR #${PR_NUMBER} に新しいレビューが到着しました（${ra}、待機${elapsed}秒）"
    report_reviews
    return 0
  fi
  return 1
}

# 漸増バックオフ: 15+30+45+60+90+120+120+120 = 600秒（約10分）
read -ra intervals <<<"${GH_WAIT_INTERVALS:-15 30 45 60 90 120 120 120}"
elapsed=0
for interval in "${intervals[@]}"; do
  check_new_review && exit 0
  if copilot_run_failed; then
    echo "ERROR: PR #${PR_NUMBER} の head でレビューの実行が失敗しています。待ってもレビューは来ません"
    echo "次の一手: /gh-actions-check ${PR_URL_CANONICAL} で原因を確認してください（レビュー用トークンの枯渇・内部エラーなど）"
    exit 7
  fi
  echo "待機中: レビュー未着（経過${elapsed}秒）。${interval}秒後に再確認します"
  sleep "$interval"
  (( elapsed += interval ))
done

check_new_review && exit 0

echo "TIMEOUT: 約10分待ちましたがPR #${PR_NUMBER} に新しいレビューが来ていません"
echo "次の一手: /gh-actions-check ${PR_URL_CANONICAL} で Actions の状況を診断してください"
exit 1
