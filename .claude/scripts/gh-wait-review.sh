#!/usr/bin/env bash
# gh-wait-review.sh — PRの自動レビュー到着を漸増バックオフで待つ
#
# 使い方: gh-wait-review.sh [PR番号]
#   PR番号省略時は現在のブランチに紐づくPRを対象とする
#
# 終了コード:
#   0 = レビュー到着
#   1 = タイムアウト（約10分）
#   2 = PRフロー適用外（gitリポジトリでない / GitHubリモートなし）
#   3 = 対象PRが見つからない
set -u

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "SKIP: gitリポジトリではありません（PRフロー適用外）"
  exit 2
fi

origin=$(git remote get-url origin 2>/dev/null || true)
if [[ "$origin" != *github.com* ]]; then
  echo "SKIP: GitHubリモートがありません（ローカルのみ・PRフロー適用外）"
  exit 2
fi

pr="${1:-$(gh pr view --json number -q .number 2>/dev/null || true)}"
if [[ -z "$pr" ]]; then
  echo "ERROR: 対象PRが見つかりません。PR番号を指定するか、PRのあるブランチで実行してください"
  exit 3
fi

review_count() {
  gh pr view "$pr" --json reviews -q '.reviews | length' 2>/dev/null || echo 0
}

report_reviews() {
  gh pr view "$pr" --json reviews -q '.reviews[] | "- \(.author.login): \(.state)"'
}

# 漸増バックオフ: 15+30+45+60+90+120+120+120 = 600秒（約10分）
elapsed=0
for interval in 15 30 45 60 90 120 120 120; do
  count=$(review_count)
  if (( count > 0 )); then
    echo "OK: PR #${pr} にレビューが到着しました（${count}件、待機${elapsed}秒）"
    report_reviews
    exit 0
  fi
  echo "待機中: レビュー未着（経過${elapsed}秒）。${interval}秒後に再確認します"
  sleep "$interval"
  (( elapsed += interval ))
done

count=$(review_count)
if (( count > 0 )); then
  echo "OK: PR #${pr} にレビューが到着しました（${count}件、待機${elapsed}秒）"
  report_reviews
  exit 0
fi

echo "TIMEOUT: 約10分待ちましたがPR #${pr} にレビューが来ていません"
echo "次の一手: /gh-actions-check ${pr} でActionsの状況を診断してください"
exit 1
