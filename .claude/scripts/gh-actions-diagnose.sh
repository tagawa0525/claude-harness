#!/usr/bin/env bash
# gh-actions-diagnose.sh — GitHub Actions の最新 run を調べ、失敗の原因を分類する
#
# Usage: gh-actions-diagnose.sh [PR番号 | ブランチ名]
#   省略時は現在のブランチ。PR 番号なら head ブランチを解決する
#
# 出力:
#   BRANCH: <branch>
#   RUNS: 直近 5 件（新しい順）
#   RUN: <id> / STATUS: <status>/<conclusion>   最新 run
#   FAILED_JOB: / FAILED_STEP:                   失敗したジョブとステップ
#   --- errors ---                              失敗ステップのログからエラー行を抜粋
#   CAUSE: <分類>
#     NONE              run がない、または最新 run が成功
#     IN_PROGRESS       最新 run が実行中
#     TRANSIENT_API     GitHub API 側の一時障害（HTTP 5xx / JSON 途切れ / RequestError）
#     COPILOT_INTERNAL  Copilot レビューの内部エラー（ccrcli / autofind cli の取得失敗）
#     CODE              上記以外（ビルド・テスト・lint の失敗）。エラー行を読んで修正する
#   NEXT: 推奨する次の一手

set -euo pipefail

TARGET="${1:-}"
PR=""
if [[ -z "$TARGET" ]]; then
  BRANCH=$(git branch --show-current 2>/dev/null || true)
  if [[ -z "$BRANCH" ]]; then
    echo "ERROR: ブランチを特定できません。PR 番号かブランチ名を指定してください" >&2
    exit 1
  fi
elif [[ "$TARGET" =~ ^[0-9]+$ ]]; then
  PR="$TARGET"
  BRANCH=$(gh pr view "$PR" --json headRefName --jq .headRefName)
else
  BRANCH="$TARGET"
fi

echo "BRANCH: $BRANCH"
[[ -n "$PR" ]] && echo "PR: $PR"

RUNS=$(gh run list --branch "$BRANCH" --limit 5 --json databaseId,name,status,conclusion,createdAt,url)
COUNT=$(jq length <<<"$RUNS")
if (( COUNT == 0 )); then
  echo "RUNS: 0"
  echo "CAUSE: NONE"
  echo "NOTE: ワークフローが未設定か、このブランチではまだ実行されていません"
  exit 0
fi

echo "RUNS:"
jq -r '.[] | "- \(.databaseId) \(.name) \(.status)/\(.conclusion) \(.createdAt) \(.url)"' <<<"$RUNS"

# gh run list は新しい順
LATEST=$(jq '.[0]' <<<"$RUNS")
ID=$(jq -r '.databaseId' <<<"$LATEST")
STATUS=$(jq -r '.status' <<<"$LATEST")
CONCLUSION=$(jq -r '.conclusion' <<<"$LATEST")
echo "RUN: $ID"
echo "STATUS: $STATUS/$CONCLUSION"

if [[ "$STATUS" != "completed" ]]; then
  echo "CAUSE: IN_PROGRESS"
  echo "NEXT: 実行中です。完了を待ってから再確認してください"
  exit 0
fi
case "$CONCLUSION" in
  success|skipped|neutral)
    echo "CAUSE: NONE"
    exit 0
    ;;
esac

JOBS=$(gh run view "$ID" --json jobs)
jq -r '.jobs[] | select(.conclusion == "failure")
  | "FAILED_JOB: \(.name)",
    (.steps[] | select(.conclusion == "failure") | "FAILED_STEP: \(.name)")' <<<"$JOBS"

# 失敗したステップのログだけを取る（ログ取得自体の失敗は空として扱い、分類は CODE に落ちる）
LOG=$(gh run view "$ID" --log-failed 2>&1 || true)
echo "--- errors ---"
grep -E '##\[error\]|error(\[|:)|Error:|FAILED|panicked' <<<"$LOG" | head -n 40 || true

if grep -qE 'HTTP 50[0-9]|Unexpected end of JSON input|RequestError|HttpError' <<<"$LOG"; then
  echo "CAUSE: TRANSIENT_API"
  echo "NEXT: GitHub API 側の一時障害です。再実行してください: gh run rerun $ID"
elif grep -qE 'Download ccrcli|Download autofind cli' <<<"$LOG"; then
  echo "CAUSE: COPILOT_INTERNAL"
  echo "NEXT: Copilot 側の一時的な問題です。再実行してください: gh run rerun $ID"
else
  echo "CAUSE: CODE"
  echo "NEXT: エラー行を読んで原因を修正してください（全文: gh run view $ID --log-failed）"
fi
