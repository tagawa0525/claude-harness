---
name: gh-actions-check
description: GitHub Actionsの実行状況を確認。Copilotレビュー待ちやCI失敗時の診断に。
model: haiku
# context: fork は廃止。fork 起動時にスキル本文が実行されず引数が失われる
# 事象を確認したため、インラインで実行する（診断のみで出力も小さい）
argument-hint: [PR番号 | ブランチ名]
allowed-tools:
  - Bash(~/.claude/scripts/gh-actions-diagnose.sh*)
  - Bash(gh run*)
---

# GitHub Actions Check Command

GitHub Actionsの実行状況を確認し、失敗時は原因を診断する。
run の取得・失敗ジョブの特定・エラー行の抽出・原因の分類はスクリプトが行う。

## 事前確認

!`gh auth status 2>&1 | head -3`

## Step 1: 診断

$ARGUMENTS（PR番号またはブランチ名。省略時は現在のブランチ）をそのまま渡す:

```bash
~/.claude/scripts/gh-actions-diagnose.sh $ARGUMENTS
```

## Step 2: CAUSE に従って対応する

| CAUSE              | 意味                                          | 対応                                                                   |
| ------------------ | --------------------------------------------- | ---------------------------------------------------------------------- |
| `NONE`             | run がない、または最新 run が成功             | `RUNS: 0` ならワークフロー未設定・未実行と報告。成功なら正常完了と報告 |
| `IN_PROGRESS`      | 最新 run が実行中                             | 待機を提案                                                             |
| `TRANSIENT_API`    | GitHub API 側の一時障害（HTTP 5xx 等）        | `NEXT:` の `gh run rerun <id>` を提案                                  |
| `COPILOT_INTERNAL` | Copilot の内部エラー（ccrcli / autofind cli） | 同上（Copilot 側の一時的な問題）                                       |
| `CODE`             | ビルド・テスト・lint の失敗                   | `--- errors ---` 以下を読み、原因と修正を提案                          |

`CODE` でエラー行だけでは原因が読み取れない場合はログ全文を確認する:

```bash
gh run view <run_id> --log-failed
```

## Step 3: 報告

```text
GitHub Actions 状況:
- Run: <RUN>
- Status: <STATUS>
- 失敗ジョブ: <FAILED_JOB>
- 失敗ステップ: <FAILED_STEP>
- 原因: <CAUSE>
- 推奨対応: <NEXT または修正案>
```
