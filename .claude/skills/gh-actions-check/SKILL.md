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

| CAUSE              | 意味                                                    | 対応                                                                                                                                                                                                                            |
| ------------------ | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `NONE`             | run がない、または最新 run と外部 CI がともに成功       | `RUNS: 0` ならワークフロー未設定・未実行と報告。成功なら正常完了と報告                                                                                                                                                          |
| `IN_PROGRESS`      | 最新 run または外部 CI が実行中                         | 待機を提案                                                                                                                                                                                                                      |
| `PERMISSION`       | 権限不足（`Resource not accessible by integration` 等） | ワークフローの `permissions:` に必要な権限を最小限で宣言する修正を提案。リポジトリ設定 `default_workflow_permissions` を write に緩める回避策は提案しない。App（Copilot 等）側の権限不足なら App のインストール設定の確認を提案 |
| `TRANSIENT_API`    | GitHub API 側の一時障害（HTTP 5xx 等）                  | `NEXT:` に従う。`ATTEMPT: 1` なら `gh run rerun <id>` を 1 回だけ提案、2 以上なら再実行せず報告                                                                                                                                 |
| `COPILOT_INTERNAL` | Copilot の内部エラー（ccrcli / autofind cli）           | 同上                                                                                                                                                                                                                            |
| `EXTERNAL`         | Actions は問題なく、外部 CI（Buildkite 等）が失敗       | `EXTERNAL_CHECK:` の URL でログを確認する。GitHub Actions の run ではないので `gh run rerun` では再実行できない                                                                                                                 |
| `CODE`             | ビルド・テスト・lint の失敗                             | `--- errors ---` 以下を読み、原因と修正を提案                                                                                                                                                                                   |

再実行は 1 回まで。`gh run rerun` は `NEXT:` が提案したときだけ実行し、`ATTEMPT: 2` 以上なら
実行しない（同じ失敗が繰り返されるなら一時障害ではない。原因を添えてユーザーの判断に委ねる）。

`CODE` でエラー行だけでは原因が読み取れない場合はログ全文を確認する:

```bash
gh run view <run_id> --log-failed
```

## Step 3: 報告

```text
GitHub Actions 状況:
- Run: <RUN>（attempt <ATTEMPT>）
- Status: <STATUS>
- 失敗ジョブ: <FAILED_JOB>
- 失敗ステップ: <FAILED_STEP>
- 外部 CI: <EXTERNAL_CHECK の一覧。なければ「なし」>
- 原因: <CAUSE>
- 推奨対応: <NEXT または修正案>
```
