---
name: gh-actions-check
description: GitHub Actionsの実行状況を確認。Copilotレビュー待ちやCI失敗時の診断に。
model: haiku
# context: fork にしない（fork では本文が実行されず、引数が失われる）
argument-hint: [PR の URL | PR番号 | ブランチ名]
allowed-tools:
  - Bash(~/.claude/scripts/gh-actions-diagnose.sh*)
  - Bash(gh run*)
---

# GitHub Actions Check Command

GitHub Actionsの実行状況を確認し、失敗時は原因を診断する。

## 事前確認

!`gh auth status 2>&1 | head -3`

## Step 1: 診断

$ARGUMENTS（PR の URL、PR番号、ブランチ名。省略時は現在のブランチ）をそのまま渡す。PR の URL 以外は cwd の
リポジトリで読むので、PR のリポジトリの worktree で実行する:

```bash
~/.claude/scripts/gh-actions-diagnose.sh "$ARGUMENTS"
```

## Step 2: NEXT に従って対応する

`NEXT:` に従う。`gh run rerun` は `NEXT:` が提案したときだけ実行する（再実行で直るのは一時の障害だけで、コードや権限の失敗は
再実行しても同じ結果になる）。

## Step 3: 報告

`--- report ---` 以下の行を報告する。`CODE` は、推奨対応に `--- errors ---` の行から読んだ原因と修正案を足す。
