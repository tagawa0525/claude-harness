#!/usr/bin/env bash
# gh-actions-diagnose.sh — CI の最新結果を調べ、失敗の原因を分類する
#
# Usage: gh-actions-diagnose.sh [PR番号 | ブランチ名]
#   省略時は現在のブランチ。PR 番号なら head ブランチと head SHA を解決する
#
# 見るもの:
#   1. GitHub Actions の run（gh run list）。失敗ならジョブ・ステップ・ログから原因を分類
#   2. head SHA の check-run のうち GitHub Actions 以外の App（Buildkite 等の外部 CI）と
#      commit status。gh run では見えず、gh run rerun でも再実行できない
#
# 出力:
#   BRANCH: <branch> / PR: <n>
#   RUNS: 直近 5 件（新しい順）
#   RUN: <id> / STATUS: <status>/<conclusion> / ATTEMPT: <n>   最新 run
#   FAILED_JOB: / FAILED_STEP:                   失敗したジョブとステップ
#   --- errors ---                              失敗ステップのログからエラー行を抜粋
#   EXTERNAL_CHECKS: <n> / EXTERNAL_CHECK: <name> <status>/<conclusion> <app> <url>
#   CAUSE: <分類>
#     NONE              run がない、または最新 run が成功（外部 CI も失敗なし）
#     IN_PROGRESS       最新 run または外部 CI が実行中
#     PERMISSION        権限不足（Resource not accessible by integration 等）。
#                       ワークフローの permissions: を最小限で宣言する。
#                       default_workflow_permissions の緩和で回避しない
#     TRANSIENT_API     GitHub API 側の一時障害（HTTP 5xx / JSON 途切れ / RequestError）
#     COPILOT_INTERNAL  Copilot レビューの内部エラー（ccrcli / autofind cli の取得失敗）
#     EXTERNAL          GitHub Actions は問題なく、外部 CI が失敗。details_url を見る
#     CODE              上記以外（ビルド・テスト・lint の失敗）。エラー行を読んで修正する
#   NEXT: 推奨する次の一手
#
# 再実行の提案は attempt 1 のときだけ。既に再実行済み（attempt 2 以上）で同じ失敗なら
# 一時障害ではない可能性が高く、無制限に再実行しても直らないため報告に切り替える。
# PERMISSION は HttpError を含むことがあるので TRANSIENT_API より先に判定する

set -euo pipefail

TARGET="${1:-}"
PR=""
HEAD_SHA=""
if [[ -z "$TARGET" ]]; then
  BRANCH=$(git branch --show-current 2>/dev/null || true)
  if [[ -z "$BRANCH" ]]; then
    echo "ERROR: ブランチを特定できません。PR 番号かブランチ名を指定してください" >&2
    exit 1
  fi
elif [[ "$TARGET" =~ ^[0-9]+$ ]]; then
  PR="$TARGET"
  PR_META=$(gh pr view "$PR" --json headRefName,headRefOid)
  BRANCH=$(jq -r '.headRefName' <<<"$PR_META")
  HEAD_SHA=$(jq -r '.headRefOid // ""' <<<"$PR_META")
else
  BRANCH="$TARGET"
fi

echo "BRANCH: $BRANCH"
[[ -n "$PR" ]] && echo "PR: $PR"

# ---------------------------------------------------------------------------
# 1. GitHub Actions
# ---------------------------------------------------------------------------
ACTIONS_CAUSE=NONE
ACTIONS_NEXT=""
ID=""

RUNS=$(gh run list --branch "$BRANCH" --limit 5 --json databaseId,name,status,conclusion,createdAt,url)
COUNT=$(jq length <<<"$RUNS")
if (( COUNT == 0 )); then
  echo "RUNS: 0"
else
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
    ACTIONS_CAUSE=IN_PROGRESS
    ACTIONS_NEXT="実行中です。完了を待ってから再確認してください"
  else
    case "$CONCLUSION" in
      success|skipped|neutral) ;;
      *)
        RUN_VIEW=$(gh run view "$ID" --json jobs,attempt)
        ATTEMPT=$(jq -r '.attempt // 1' <<<"$RUN_VIEW")
        echo "ATTEMPT: $ATTEMPT"
        jq -r '.jobs[] | select(.conclusion == "failure")
          | "FAILED_JOB: \(.name)",
            (.steps[] | select(.conclusion == "failure") | "FAILED_STEP: \(.name)")' <<<"$RUN_VIEW"

        # 失敗したステップのログだけを取る（ログ取得自体の失敗は空として扱い、分類は CODE に落ちる）
        LOG=$(gh run view "$ID" --log-failed 2>&1 || true)
        echo "--- errors ---"
        grep -E '##\[error\]|error(\[|:)|Error:|FAILED|panicked' <<<"$LOG" | head -n 40 || true

        # 再実行を提案してよいのは初回の失敗だけ
        rerun_or_report() {
          local what="$1"
          if (( ATTEMPT >= 2 )); then
            ACTIONS_NEXT="${what}ですが、既に ${ATTEMPT} 回試行して失敗しています。これ以上再実行せず、ログ（gh run view $ID --log-failed）を添えてユーザーに報告し判断を仰いでください"
          else
            ACTIONS_NEXT="${what}です。1 回だけ再実行してください: gh run rerun $ID"
          fi
        }

        if grep -qiE 'not accessible by (integration|personal access token)|refusing to allow a GitHub App|permission denied|permission_denied|insufficient permission|does not have (the )?permission|HTTP 403|403 Forbidden|status code 403' <<<"$LOG"; then
          ACTIONS_CAUSE=PERMISSION
          ACTIONS_NEXT="権限不足です。ワークフローの permissions: に必要な権限を最小限で宣言してください（例: pull-requests: write）。リポジトリ設定の default_workflow_permissions を write に緩めて回避しないこと。GitHub App（Copilot 等）側の権限不足なら App のインストール設定を確認してください"
        elif grep -qE 'HTTP 50[0-9]|Unexpected end of JSON input|RequestError|HttpError' <<<"$LOG"; then
          ACTIONS_CAUSE=TRANSIENT_API
          rerun_or_report "GitHub API 側の一時障害"
        elif grep -qE 'Download ccrcli|Download autofind cli' <<<"$LOG"; then
          ACTIONS_CAUSE=COPILOT_INTERNAL
          rerun_or_report "Copilot 側の一時的な問題"
        else
          ACTIONS_CAUSE=CODE
          ACTIONS_NEXT="エラー行を読んで原因を修正してください（全文: gh run view $ID --log-failed）"
        fi
        ;;
    esac
  fi
fi

# ---------------------------------------------------------------------------
# 2. 外部 CI（GitHub Actions 以外の App の check-run と commit status）
# ---------------------------------------------------------------------------
EXT_FAILED=0
EXT_PENDING=0
EXT_AVAILABLE=0

if [[ -z "$HEAD_SHA" ]]; then
  HEAD_SHA=$(gh api "repos/{owner}/{repo}/git/ref/heads/${BRANCH}" --jq '.object.sha' 2>/dev/null || true)
fi
if [[ -n "$HEAD_SHA" ]]; then
  # gh の成否で先に分ける。`gh … | jq -s` を || true で受けると、gh が失敗しても jq が
  # 空入力から [] を作り「外部 CI なし」と誤認するため、gh の出力を先に受け取ってから整形する
  CHECKS=""
  STATUSES=""
  if CHECKS_RAW=$(gh api --paginate "repos/{owner}/{repo}/commits/${HEAD_SHA}/check-runs?per_page=100" \
      --jq '.check_runs[] | select(.app.slug != "github-actions")
            | {name, status, conclusion, app: .app.slug, url: .details_url}' 2>/dev/null); then
    CHECKS=$(jq -s '.' <<<"$CHECKS_RAW")
  fi
  if STATUSES_RAW=$(gh api "repos/{owner}/{repo}/commits/${HEAD_SHA}/status" \
      --jq '[.statuses[] | {name: .context,
                            status: (if .state == "pending" then "in_progress" else "completed" end),
                            conclusion: .state, app: "commit-status", url: .target_url}]' 2>/dev/null); then
    STATUSES="$STATUSES_RAW"
  fi
  if jq -e 'type == "array"' <<<"$CHECKS" >/dev/null 2>&1 && jq -e 'type == "array"' <<<"$STATUSES" >/dev/null 2>&1; then
    EXT_AVAILABLE=1
    EXTERNAL=$(jq -s 'add' <<<"$CHECKS $STATUSES")
    echo "EXTERNAL_CHECKS: $(jq 'length' <<<"$EXTERNAL")"
    jq -r '.[] | "EXTERNAL_CHECK: \(.name) \(.status)/\(.conclusion) \(.app) \(.url // "-")"' <<<"$EXTERNAL"
    EXT_PENDING=$(jq '[.[] | select(.status != "completed")] | length' <<<"$EXTERNAL")
    EXT_FAILED=$(jq '[.[] | select(.status == "completed" and (.conclusion | IN("success","neutral","skipped") | not))] | length' <<<"$EXTERNAL")
    EXT_FAILED_LIST=$(jq -r '[.[] | select(.status == "completed" and (.conclusion | IN("success","neutral","skipped") | not)) | "\(.name) (\(.app)): \(.url // "-")"] | join(", ")' <<<"$EXTERNAL")
  fi
fi
if (( ! EXT_AVAILABLE )); then
  echo "EXTERNAL_CHECKS: unavailable"
  echo "NOTE: head SHA の check-run を取得できません（ブランチが未 push か、API エラー）"
fi

# ---------------------------------------------------------------------------
# 3. 総合判定。GitHub Actions 側に原因があればそれを優先し、外部 CI は情報として添える
# ---------------------------------------------------------------------------
if [[ "$ACTIONS_CAUSE" != "NONE" ]]; then
  echo "CAUSE: $ACTIONS_CAUSE"
  echo "NEXT: $ACTIONS_NEXT"
elif (( EXT_FAILED > 0 )); then
  echo "CAUSE: EXTERNAL"
  echo "NEXT: 外部 CI の失敗です（GitHub Actions の run ではないため、ここからは再実行できません）。ログを details_url で確認し、一時的な失敗なら外部 CI 側で再実行してください: ${EXT_FAILED_LIST}"
elif (( EXT_PENDING > 0 )); then
  echo "CAUSE: IN_PROGRESS"
  echo "NEXT: 外部 CI が実行中です。完了を待ってから再確認してください"
else
  echo "CAUSE: NONE"
  if (( COUNT == 0 )); then
    echo "NOTE: ワークフローが未設定か、このブランチではまだ実行されていません"
  fi
fi
