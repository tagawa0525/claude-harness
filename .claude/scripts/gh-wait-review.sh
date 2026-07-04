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
