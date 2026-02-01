---
name: git-worktree
description: |
  並行作業用に別ディレクトリで作業環境を作成。複数PRの同時進行に最適。
  /git-worktree <branch-name> で作成、--remove で削除。
model: haiku
argument-hint: <branch-name> [--remove]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git worktree*)
  - Bash(git switch*)
  - Bash(ls*)
  - Bash(pwd*)
---

# Git Worktree Command

並行作業用に別ディレクトリで作業環境を作成する。

## 現在の状態

!`git worktree list`
!`git branch -vv`

## 使用シナリオ

- 複数のPRを同時に進める
- レビュー待ちの間に別作業
- 長時間かかる作業を中断せず別タスク
- 本番環境の緊急修正（既存作業を保持したまま）

## 操作モード

### 新規worktree作成（新規ブランチ）

$ARGUMENTS にブランチ名が指定されている場合:

```bash
# 親ディレクトリに worktree を作成
git worktree add ../[repo-name]-[branch-name] -b [branch-name]
```

ディレクトリ名の例:

- リポジトリが `myapp` でブランチが `feat/login` の場合
- → `../myapp-feat-login`

### 新規worktree作成（既存ブランチ）

既存のリモートブランチをチェックアウトする場合:

```bash
git worktree add ../[dir-name] [branch-name]
```

### worktree一覧

```bash
git worktree list
```

### worktree削除（--remove 指定時）

```bash
# worktree を削除
git worktree remove [path]

# pruneで不要な参照をクリーンアップ
git worktree prune
```

## 完了メッセージ

### 作成時

```text
✅ worktree を作成しました。

場所: ../[dir-name]
ブランチ: [branch-name]

作業を開始するには:
  cd ../[dir-name]

※ このディレクトリと並行して作業できます
```

### 削除時

```text
✅ worktree を削除しました: [path]

現在の worktree:
[git worktree list の出力]
```

## 注意事項

- 同じブランチを複数のworktreeでチェックアウトすることはできない
- worktreeのディレクトリを手動で削除した場合は `git worktree prune` が必要
- マージ済みブランチのworktreeは `/git-info` で検出される

## 関連コマンド

- `/git-branch` - 通常のブランチ切り替え（同じディレクトリ）
- `/git-info` - 不要なworktreeの検出
- `/git-merge` - マージ後にworktreeを自動削除

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合（ブランチ名など）は通常のテキスト質問
