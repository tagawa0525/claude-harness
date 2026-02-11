---
name: gh-actions-check
description: GitHub Actionsの実行状況を確認。Copilotレビュー待ちやCI失敗時の診断に。
model: haiku
argument-hint: [PR番号 | ブランチ名]
allowed-tools:
  - Bash(gh *)
  - Bash(git branch*)
---

# GitHub Actions Check Command

GitHub Actionsの実行状況を確認し、失敗時は原因を診断する。

## 事前確認

!`gh auth status 2>&1 | head -3`

## Step 1: 対象の特定

$ARGUMENTS にPR番号またはブランチ名が指定されている場合はそれを使用。
指定がない場合は現在のブランチを使用。

```bash
git branch --show-current
```

## Step 2: Actions実行一覧の確認

```bash
# ブランチ指定の場合
gh run list --branch <branch> --limit 5

# PR番号指定の場合（厳密なフィルタ）
gh run list --limit 50 \
  --json databaseId,headBranch,status,conclusion,workflowName,createdAt \
  --jq '.[]
    | select(.headBranch | startswith("refs/pull/<pr_number>/"))
    | {run_id: .databaseId, workflow: .workflowName, status, conclusion,
       created: .createdAt}'
```

確認ポイント:

- `completed/success` → 正常完了
- `completed/failure` → Step 3で診断
- `in_progress` → 実行中（待機）
- 出力が空 → ワークフロー未設定

## Step 3: 失敗ジョブの特定

```bash
gh run view <run_id> --json jobs \
  --jq '.jobs[]
    | select(.conclusion=="failure")
    | {name, steps: [.steps[]
      | select(.conclusion=="failure")
      | .name]}'
```

## Step 4: 失敗ステップの詳細ログ

```bash
gh run view <run_id> --log 2>&1 | grep -B 2 -A 10 "##\[error\]"
```

## Step 5: 原因の分類と対応

### GitHub API側の一時障害

- HTTP 500, 502, 503 エラー
- `Unexpected end of JSON input`
- `RequestError`, `HttpError`

→ 再実行を提案: `gh run rerun <run_id>`

### Copilot内部エラー

- `Download ccrcli` 失敗
- `Download autofind cli` 失敗

→ 再実行を提案（Copilot側の一時的な問題）

### コード起因のエラー

- ビルド失敗、テスト失敗、lint違反

→ エラー内容を報告し修正を提案

## Step 6: 報告

```text
GitHub Actions 状況:
- Run: <run_id>
- Status: <status>/<conclusion>
- 失敗ジョブ: <job_name>
- 失敗ステップ: <step_name>
- 原因: <分類>
- 推奨対応: <対応>
```

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合は通常のテキスト質問
