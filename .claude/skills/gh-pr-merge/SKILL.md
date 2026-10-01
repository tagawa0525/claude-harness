---
name: gh-pr-merge
description: GitHub PRをマージ。マージ後のブランチ削除やworktreeクリーンアップも対応。
# マージコミットメッセージ（Why/What/Impact）生成の品質を優先
model: sonnet
argument-hint: [PR番号]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(gh pr*)
  - Bash(gh auth*)
  - Bash(~/.claude/scripts/gh-wait-review.sh*)
  - Bash(~/.claude/scripts/post-merge-cleanup.sh*)
---

# GitHub PR Merge Command

GitHub Pull Requestをマージする。マージ方式・本文の見出し・CI・未解決スレッド・
head の遅れなど機械的に判定できる条件は `pre-merge-check` hook が検査し、満たさなければ
理由と直し方を示して deny する。手順で扱うのは判断が要るものだけ。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git branch -vv`

## マージ前の判断

$ARGUMENTS の PR（省略時は現在のブランチの PR）について:

- 自動レビューが未着なら `~/.claude/scripts/gh-wait-review.sh [PR番号]` で待つ（約 10 分。
  Bash ツールの `run_in_background=true`）。タイムアウト（exit 1）は /gh-actions-check で診断
- 指摘への対応が残っていれば /gh-pr-review に戻る
- 周回上限到達で未確認の対応・見送りが残っていれば、一覧を提示してユーザーの承認を得る

## マージコミットメッセージ

PR の内容を把握してから書く:

```bash
gh pr view [PR番号] --json title,body,commits,files
```

```text
Merge: [PRタイトルを簡潔に要約]

## Why
[PRの目的・背景を1-2文]

## What
- [主要な変更点を3-5項目]

## Impact
[影響するモジュール/機能]

PR: #[番号]
```

## マージ実行

```bash
gh pr merge [PR番号] --merge --delete-branch \
  --subject "Merge: [subject]" \
  --body "[body]"
```

## クリーンアップ

```bash
~/.claude/scripts/post-merge-cleanup.sh [headブランチ名]
```

worktree 削除 → main へ切り替えて pull → ローカル・リモートのブランチ削除までを行う。
そのブランチを head とする open PR（fork からの upstream PR 等）があればリモートは残す
（`REMOTE_BRANCH: kept`）。未コミットの変更や履歴の分岐で失敗したら原因を確認して対処する。

## 完了メッセージ

```text
✅ PRをマージしました。

クリーンアップ完了:
- worktree [path] を削除（該当する場合）
- ブランチ [branch] を削除（リモートを残した場合はその理由）
```
