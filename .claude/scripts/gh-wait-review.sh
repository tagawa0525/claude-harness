#!/usr/bin/env bash
# gh-wait-review.sh — PRの自動レビュー到着を漸増バックオフで待つ
#
# 使い方: gh-wait-review.sh [PR番号]
#   PR番号省略時は現在のブランチに紐づくPRを対象とする
#
# 検出対象は2種類:
#   1. 新しいレビュー提出（reviews の件数増加）
#   2. Copilot からの新しいPRコメント（issue comment の件数増加）
#      再レビュー依頼に対し Copilot は正式なレビューを提出せず
#      「対応を確認しました」等のコメントだけで応答することがあるため
#
# 終了コード:
#   0 = レビューまたはCopilotコメント到着
#   1 = タイムアウト（約10分）
#   2 = PRフロー適用外（gitリポジトリでない / GitHubリモートなし）
#   3 = 対象PRが見つからない
#   4 = gh が利用できない（未インストール / 未認証）
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

pr="${1:-}"
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

review_count() {
  gh pr view "$pr" --json reviews -q '.reviews | length' 2>/dev/null || echo 0
}

# Copilot からのPRコメント（issue comment）のID列（作成順）。
# 自分やレビュアー以外のコメントで誤検知しないよう作者をcopilotに限定する。
# コメント100件超のPRで新着を取りこぼさないよう --paginate で全ページを走査する
copilot_comment_ids() {
  gh api --paginate "repos/{owner}/{repo}/issues/${pr}/comments?per_page=100" \
    --jq '.[] | select(.user.login | ascii_downcase | contains("copilot")) | .id' \
    2>/dev/null
}

# ghの失敗は空出力→0件になり、既存の「失敗を0件扱い」の挙動を維持する
copilot_comment_count() {
  copilot_comment_ids | wc -l
}

report_reviews() {
  gh pr view "$pr" --json reviews -q '.reviews[] | "- \(.author.login): \(.state)"'
}

# 最新のCopilotコメントの冒頭を表示する（本文全体は gh pr view で確認）
report_latest_copilot_comment() {
  local id
  id=$(copilot_comment_ids | tail -n 1)
  [[ -n "$id" ]] || return 0
  gh api "repos/{owner}/{repo}/issues/comments/${id}" \
    --jq '"- \(.user.login) (\(.created_at)):\n\(.body)"' \
    2>/dev/null | head -12
}

# 開始時点の件数を基準にし、増加をもって「新しい応答の到着」と判定する
# （指摘対応後の再レビュー待ちで、過去のレビュー・コメントを誤検知しないため）
review_baseline=$(review_count)
comment_baseline=$(copilot_comment_count)
if (( review_baseline > 0 || comment_baseline > 0 )); then
  echo "INFO: 既存レビュー${review_baseline}件・Copilotコメント${comment_baseline}件。新しい応答の到着を待ちます"
fi

# 新しいレビューまたはCopilotコメントが到着していれば報告して成功(0)、未着なら失敗(1)
check_new_responses() {
  local rc cc
  rc=$(review_count)
  if (( rc > review_baseline )); then
    echo "OK: PR #${pr} に新しいレビューが到着しました（計${rc}件、待機${elapsed}秒）"
    report_reviews
    return 0
  fi
  cc=$(copilot_comment_count)
  if (( cc > comment_baseline )); then
    echo "OK: PR #${pr} に新しいCopilotコメントが到着しました（計${cc}件、待機${elapsed}秒）"
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
