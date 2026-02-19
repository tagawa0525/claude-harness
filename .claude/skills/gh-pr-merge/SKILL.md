---
name: gh-pr-merge
description: GitHub PRをマージ。マージ後のブランチ削除やworktreeクリーンアップも対応。
model: haiku
argument-hint: [PR番号] [--delete]
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

# GitHub PR Merge Command

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

## マージコミットメッセージの生成

マージ前にPRの情報を収集し、意味のあるマージコミットメッセージを生成する。

### 1. PR情報の収集

```bash
# PRの詳細情報を取得
gh pr view [PR番号] --json title,body,commits,files,additions,deletions

# コミット一覧を確認
gh pr view [PR番号] --json commits --jq '.commits[].messageHeadline'
```

### 2. マージコミットメッセージの作成

以下の構造でマージコミットメッセージを作成する：

**Subject行（1行目）:**

```text
Merge: [PRタイトルを簡潔に要約]
```

**Body（本文）:**

```text
## Why（なぜこの変更が必要か）
[PRの目的・背景を1-2文で説明]

## What（何が変わるか）
[主要な変更点を箇条書きで3-5項目]

## Impact（影響範囲）
[どのモジュール/機能に影響するか]

PR: #[番号]
```

### 3. マージコミットの良い例・悪い例

**❌ 悪い例（GitHubデフォルト）:**

```text
Merge pull request #42 from user/fix-typo
```

**✅ 良い例:**

```text
Merge: ユーザー認証のタイムアウト処理を修正

## Why
セッションタイムアウト時にユーザーが無限ループに陥るバグがあった

## What
- セッション期限切れ時のリダイレクト処理を追加
- エラーメッセージを日本語化
- タイムアウト値を環境変数で設定可能に

## Impact
認証関連のコンポーネント（Login, Session, AuthGuard）

PR: #42
```

## マージ実行

### マージコミット方式のみ（必須）

**マージコミットを作成してPRの履歴を保持する。**

```bash
gh pr merge [PR番号] --merge \
  --subject "Merge: [生成したsubject]" \
  --body "[生成したbody]"
```

⚠️ **squash、rebase は基本禁止**。マージの記録を残すため、常にマージコミット方式を使用する。

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
# ブランチが存在する場合のみ削除
git branch -d [branch] 2>/dev/null || true
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
