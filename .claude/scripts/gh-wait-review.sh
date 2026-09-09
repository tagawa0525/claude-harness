#!/usr/bin/env bash
# gh-wait-review.sh — PRの自動レビュー到着を漸増バックオフで待つ
#
# 使い方: gh-wait-review.sh [PR番号] [--since <ISO 8601 時刻>]
#   PR番号省略時は現在のブランチに紐づくPRを対象とする
#   --since 省略時は「最後の Copilot へのレビュー要求」を基準にし、それ以降に提出された
#   レビューを待つ。要求が 1 件も無ければ待たずにエラーで終わる（要求していない
#   レビューは来ないため）
#   --since 指定時はその時刻を基準にする（request-rereview.sh が要求の created_at を渡す）
#
# 基準を「起動時点の最新レビュー」にすると、直前に届いたレビューを基準にして
# 「絶対に来ない次のレビュー」を 10 分待ててしまう。実際に、要求と待機を続けて
# 実行したセッションが空待ちを繰り返し、届いたレビューに気づけなかった
# （2026-09-08）。基準を要求に紐づけると、要求後のレビューが既にあれば即座に
# 成功して終わるので、続けて呼んでも無害になる
#
# 検出するのは基準時刻より新しいレビュー提出（submittedAt）だけ。Copilot の
# PRコメントは待機の打ち切り条件にしない。@copilot メンションに応答するのは
# copilot-swe-agent（コーディングエージェント）で、「対応を確認しました」等の
# コメントを返してもレビューは提出されない。これを応答として扱っていたため、
# レビューが来ていないのに「応答あり」で先へ進んでいた（2026-08-23〜09-08）
#
# 基準は件数ではなく GitHub 側の時刻で持つ。ローカル時計は使わない
#
# 待機間隔（秒）は GH_WAIT_INTERVALS で上書きできる（テスト用）
#
# 終了コード:
#   0 = レビュー到着
#   1 = タイムアウト（約10分）
#   2 = PRフロー適用外（gitリポジトリでない / GitHubリモートなし）
#   3 = 対象PRが見つからない
#   4 = gh が利用できない（未インストール / 未認証）
#   5 = 引数エラー（不明なオプション / --since の値なし / PR番号の重複指定）
#   6 = レビュー要求がない（待っても来ない）、または要求を取得できない
#   7 = レビューの実行が失敗している（トークン切れ・内部エラー等。待っても来ない）
set -u

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "SKIP: gitリポジトリではありません（PRフロー適用外）"
  exit 2
fi

# いずれかのリモートにGitHubがあればPRフロー対象（originという名前には依存しない）
if ! git remote -v 2>/dev/null | grep -q 'github\.com'; then
  echo "SKIP: GitHubリモートがありません（ローカルのみ・PRフロー適用外）"
  exit 2
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

pr=""
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
      if [[ -n "$pr" ]]; then
        echo "ERROR: PR番号は1つだけ指定してください: '$pr' と '$1'"
        exit 5
      fi
      pr="$1"
      shift
      ;;
  esac
done
if [[ -z "$pr" ]]; then
  if ! pr=$(gh pr view --json number -q .number 2>/dev/null) || [[ -z "$pr" ]]; then
    echo "ERROR: 現在のブランチに紐づくPRが見つかりません。PR番号を指定してください"
    exit 3
  fi
fi

# ポーリング中のカウント関数はghの失敗を0件扱いするため、
# PRの存在はここで一度だけ検証し「PRなし」がtimeoutに化けるのを防ぐ
if ! gh pr view "$pr" --json number >/dev/null 2>&1; then
  echo "ERROR: PR #${pr} が見つからないか、アクセスできません"
  exit 3
fi

# 最新レビューの提出時刻（ISO 8601、レビューが無ければ空）
latest_review_at() {
  gh pr view "$pr" --json reviews -q '[.reviews[].submittedAt] | max // ""' 2>/dev/null || echo ""
}

# head の Copilot チェックが失敗しているか（トークン切れ・内部エラーでレビューが
# 走らなかった場合。待っても来ないので、10分待たずに切り上げるために見る）。
# head は待機中の push で変わるので毎回取り直す（古い head の失敗で打ち切らない）。
# 取得できないときは「失敗していない」として待機を続ける（誤って打ち切らない）
copilot_run_failed() {
  local head_sha names
  head_sha=$(gh pr view "$pr" --json headRefOid -q '.headRefOid' 2>/dev/null) || return 1
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
  gh pr view "$pr" --json reviews -q '.reviews[] | "- \(.author.login): \(.state)"'
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
  if ! requests=$("$requests_script" "$pr"); then
    echo "ERROR: PR #${pr} のレビュー要求を取得できません（timeline API が失敗）"
    exit 6
  fi
  since=$(tail -n 1 <<<"$requests")
  if [[ -z "$since" ]]; then
    echo "ERROR: PR #${pr} には Copilot へのレビュー要求がありません。要求していないレビューは来ないので待ちません"
    # 配備版（~/.claude）の旧版を案内しないよう、自分と同じツリーのスクリプトを指す
    echo "次の一手: $(cd "$(dirname "$requests_script")/.." && pwd)/skills/gh-pr-review/scripts/request-rereview.sh ${pr} でレビューを要求してください"
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
    echo "OK: PR #${pr} に新しいレビューが到着しました（${ra}、待機${elapsed}秒）"
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
    echo "ERROR: PR #${pr} の head でレビューの実行が失敗しています。待ってもレビューは来ません"
    echo "次の一手: /gh-actions-check ${pr} で原因を確認してください（レビュー用トークンの枯渇・内部エラーなど）"
    exit 7
  fi
  echo "待機中: レビュー未着（経過${elapsed}秒）。${interval}秒後に再確認します"
  sleep "$interval"
  (( elapsed += interval ))
done

check_new_review && exit 0

echo "TIMEOUT: 約10分待ちましたがPR #${pr} に新しいレビューが来ていません"
echo "次の一手: /gh-actions-check ${pr} でActionsの状況を診断してください"
exit 1
