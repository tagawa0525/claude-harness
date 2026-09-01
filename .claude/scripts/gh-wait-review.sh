#!/usr/bin/env bash
# gh-wait-review.sh — PRの自動レビュー到着を漸増バックオフで待つ
#
# 使い方: gh-wait-review.sh [PR番号] [--since <ISO 8601 時刻>]
#   PR番号省略時は現在のブランチに紐づくPRを対象とする
#   --since 省略時は起動時点で最新の応答の時刻を基準にし、それより新しい応答を待つ
#   --since 指定時はその時刻より新しい応答を待つ（再レビュー依頼コメントの
#   created_at を渡せば、依頼投稿と待機開始の間に届いた応答も取りこぼさない）
#
# 検出対象は2種類（いずれも基準時刻より新しいもの）:
#   1. 新しいレビュー提出（submittedAt）
#   2. Copilot からの新しいPRコメント（issue comment の created_at）
#      再レビュー依頼に対し Copilot は正式なレビューを提出せず
#      「対応を確認しました」等のコメントだけで応答することがあるため
#
# 基準は件数ではなく GitHub 側の時刻で持つ。ローカル時計は使わない
#
# 終了コード:
#   0 = レビューまたはCopilotコメント到着
#   1 = タイムアウト（約10分）
#   2 = PRフロー適用外（gitリポジトリでない / GitHubリモートなし）
#   3 = 対象PRが見つからない
#   4 = gh が利用できない（未インストール / 未認証）
#   5 = 引数エラー（不明なオプション / --since の値なし / PR番号の重複指定）
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

# Copilot からのPRコメント（issue comment）の「created_at id」列（作成順）。
# 部分一致では my-copilot 等の無関係ユーザーを誤検知するため、
# Bot種別かつ既知のCopilotログイン名への完全一致に限定する
# （同一コメントでも list API は "Copilot"、単体取得は
#   "copilot-swe-agent[bot]" と表記が揺れるため両方を許可リストに含める）。
# コメント100件超のPRで新着を取りこぼさないよう --paginate で全ページを走査する
copilot_comments() {
  gh api --paginate "repos/{owner}/{repo}/issues/${pr}/comments?per_page=100" \
    --jq '.[] | select(.user.type == "Bot"
                       and ((.user.login | ascii_downcase)
                            | IN("copilot", "copilot-swe-agent[bot]", "copilot-pull-request-reviewer[bot]")))
             | "\(.created_at) \(.id)"' \
    2>/dev/null
}

# 最新の Copilot コメントの作成時刻（無ければ空）。ghの失敗は空→「未着」扱い
latest_copilot_comment_at() {
  copilot_comments | tail -n 1 | cut -d' ' -f1
}

report_reviews() {
  gh pr view "$pr" --json reviews -q '.reviews[] | "- \(.author.login): \(.state)"'
}

# 最新のCopilotコメントの冒頭を表示する（本文全体は gh pr view で確認）
report_latest_copilot_comment() {
  local id
  id=$(copilot_comments | tail -n 1 | cut -d' ' -f2)
  [[ -n "$id" ]] || return 0
  gh api "repos/{owner}/{repo}/issues/comments/${id}" \
    --jq '"- \(.user.login) (\(.created_at)):\n\(.body)"' \
    2>/dev/null | head -12
}

# 基準時刻より新しい応答の到着をもって「新しい応答」と判定する
# （指摘対応後の再レビュー待ちで、過去のレビュー・コメントを誤検知しないため）。
# --since 未指定なら起動時点の最新応答の時刻を基準にする（ISO 8601 は文字列比較で時系列順）
if [[ -z "$since" ]]; then
  r0=$(latest_review_at)
  c0=$(latest_copilot_comment_at)
  since="$r0"
  [[ "$c0" > "$since" ]] && since="$c0"
  if [[ -n "$since" ]]; then
    echo "INFO: 既存の最新応答は ${since}。それより新しい応答の到着を待ちます"
  fi
else
  echo "INFO: ${since} より新しい応答の到着を待ちます"
fi

# 新しいレビューまたはCopilotコメントが到着していれば報告して成功(0)、未着なら失敗(1)
check_new_responses() {
  local ra ca
  ra=$(latest_review_at)
  if [[ -n "$ra" && "$ra" > "$since" ]]; then
    echo "OK: PR #${pr} に新しいレビューが到着しました（${ra}、待機${elapsed}秒）"
    report_reviews
    return 0
  fi
  ca=$(latest_copilot_comment_at)
  if [[ -n "$ca" && "$ca" > "$since" ]]; then
    echo "OK: PR #${pr} に新しいCopilotコメントが到着しました（${ca}、待機${elapsed}秒）"
    echo "NOTE: 正式なレビュー提出ではないため、内容を読んで対応要否を判断してください"
    report_latest_copilot_comment
    return 0
  fi
  return 1
}

# 漸増バックオフ: 15+30+45+60+90+120+120+120 = 600秒（約10分）
elapsed=0
for interval in 15 30 45 60 90 120 120 120; do
  check_new_responses && exit 0
  echo "待機中: 応答未着（経過${elapsed}秒）。${interval}秒後に再確認します"
  sleep "$interval"
  (( elapsed += interval ))
done

check_new_responses && exit 0

echo "TIMEOUT: 約10分待ちましたがPR #${pr} に新しいレビュー・Copilotコメントが来ていません"
echo "次の一手: /gh-actions-check ${pr} でActionsの状況を診断してください"
exit 1
