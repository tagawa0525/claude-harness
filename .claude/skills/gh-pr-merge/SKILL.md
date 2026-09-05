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

以下は `pre-merge-check` hook が `gh pr merge` 実行時に機械的に検証し、
満たさなければ deny する（手順で確認するのは判断が必要な項目だけでよい）:

- `--merge` 指定・`--squash` / `--rebase` なし
- `--delete-branch` あり
- `--body`（または `--body-file`）に `## Why` / `## What` / `## Impact`
- CI チェックが未完了・失敗でない、reviewDecision が CHANGES_REQUESTED / REVIEW_REQUIRED でない
- 未解決のレビュースレッドがない（対応後は `resolve-thread.sh` で resolve する）
- head が base より遅れていない（`git fetch && git rebase origin/main && git push --force-with-lease` で整えてからマージする）

- CIステータス: 全てパスしているか
- レビュー: Copilot等の自動レビューが完了し、指摘事項に対応済みか
  - 未着の場合は `~/.claude/scripts/gh-wait-review.sh [PR番号]` で待機（漸増バックオフで約10分。フォアグラウンドの最大タイムアウトを超えるため、必ずバックグラウンドで実行）
  - スクリプトは新しいレビュー提出と Copilot の PR コメント応答の両方を検出する。コメント検出時（`NOTE:` 行付き）は内容を読んで承認相当（「対応を確認しました」等）か追加指摘かを判断する
  - タイムアウト（exit 1）時は /gh-actions-check で診断
  - ただし /gh-pr-review が「Suppressed comments のみ」または「周回上限到達」で
    終了した場合、最後の push への再レビューは**意図的に依頼していない**ので
    待機しない（待っても来ないため、タイムアウトをトリガー失敗と誤診しない）
  - 周回上限到達で対応・見送りした指摘が未確認のまま残っている場合は、
    その一覧を提示してユーザーの承認を得てからマージする
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

⚠️ **squash、rebase は基本禁止**。マージの記録を残すため、常にマージコミット方式を使用する。

```bash
gh pr merge [PR番号] --merge \
  --subject "Merge: [生成したsubject]" \
  --body "[生成したbody]"
```

## クリーンアップ

マージ完了後、PR の head ブランチ名を渡して実行する:

```bash
~/.claude/scripts/post-merge-cleanup.sh [branch]
```

スクリプトが順に行う: worktree の削除 → main へ切り替えて `fetch --prune` / `pull --ff-only` →
ローカルブランチ削除 → リモートブランチ削除。そのブランチを head とする open PR
（fork からの upstream PR 等）があれば、削除で PR が閉じるためリモートは残す（`REMOTE_BRANCH: kept`）。
worktree に未コミットの変更が残っている、main とリモートの履歴が分岐している、といった場合は
その場で失敗するので、原因を確認して対処する。

## 完了確認

```bash
gh pr view [PR番号] --json state,mergedAt,mergedBy
git log --oneline -5
```

## 完了メッセージ

```text
✅ PRをマージしました。

クリーンアップ完了:
- worktree [path] を削除（該当する場合）
- ブランチ [branch] を削除（リモートを残した場合はその理由）

現在の状態を確認: /git-info
```
