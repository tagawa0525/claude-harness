---
model: haiku
argument-hint: [PR番号] [--squash] [--rebase] [--delete]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git fetch*)
  - Bash(git pull*)
  - Bash(git worktree*)
  - Bash(gh pr*)
  - Bash(gh auth*)
  - Bash(rm -rf*)
---

# Git Merge Command

GitHub Pull Requestをマージする（gh CLI使用）。

## 事前確認

!`gh auth status`
!`git branch -vv`

## PR状態の確認

$ARGUMENTS にPR番号が指定されている場合はそのPRを対象。
指定がない場合は現在のブランチに関連するPRを検索。

```bash
gh pr status
gh pr view [PR番号] --json state,title,mergeable,reviewDecision,headRefName
```

### マージ可能性チェック

- CIステータス: 全てパスしているか
- レビュー: 承認されているか
- コンフリクト: なしか

## マージ実行

### 通常マージ（マージコミット作成）

```bash
gh pr merge [PR番号] --merge
```

### --squash オプション（コミットを1つにまとめる）

```bash
gh pr merge [PR番号] --squash
```

feature ブランチの細かいコミットを1つにまとめたい場合に使用。

### --rebase オプション（リベースマージ）

```bash
gh pr merge [PR番号] --rebase
```

直線的な履歴を保ちたい場合に使用。

## ブランチ削除（--delete 指定時）

マージ後にリモートブランチを削除:

```bash
gh pr merge [PR番号] --delete-branch
```

## ローカルの同期とクリーンアップ

マージ完了後:

### 1. ローカルを最新化

```bash
git fetch --prune
git switch main
git pull
```

### 2. 関連worktreeの削除

マージされたブランチに関連するworktreeがあれば削除:

```bash
# worktree一覧を確認
git worktree list

# 該当するworktreeがあれば削除
git worktree remove [path]
```

### 3. ローカルブランチの削除

```bash
git branch -d [branch]
```

## 完了確認

```bash
gh pr view [PR番号] --json state,mergedAt,mergedBy
git log --oneline -5
git worktree list
```

## 完了メッセージ

```text
✅ PRをマージしました。

クリーンアップ完了:
- ローカルブランチ [branch] を削除
- worktree [path] を削除（該当する場合）

現在の状態を確認: /git-info
```
