---
name: gh-pr-review
description: |
  PRレビューコメントを確認し対応。コード修正、返信、resolve を実行。
  /gh-pr-review [PR番号|コメントURL] で呼び出し。--unresolved で未解決のみ表示。
model: sonnet
argument-hint: [PR番号 | コメントURL] [--unresolved]
allowed-tools:
  - Bash(git *)
  - Bash(gh *)
  - Read
  - Edit
  - Glob
  - Grep
  - WebSearch
  - WebFetch
  - Task
---

# GitHub PR Review Command

PRについたレビューコメントを確認し、対応する。

## ヘルパースクリプト

このスキルは `scripts/` ディレクトリのスクリプトを使用:

- `get-pr-info.sh [pr_number]` - PR情報を取得
- `get-review-comments.sh <pr_number> [--unresolved]` - レビューコメントを取得
- `reply-to-comment.sh <pr_number> <comment_id> <body>` - コメントに返信
- `resolve-thread.sh <pr_number> <thread_node_id>` - スレッドを解決

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv | head -5`

## Step 1: 引数の解析

### パターン A: コメントURL が指定された場合

URL形式: `https://github.com/{owner}/{repo}/pull/{pr_number}#discussion_r{comment_id}`

→ 特定のコメントに対応

### パターン B: PR番号が指定された場合

→ そのPRの全レビューコメントを取得

### パターン C: 引数なし

→ 現在のブランチに関連するPRを特定

```bash
.claude/skills/gh-pr-review/scripts/get-pr-info.sh
```

---

## Step 2: レビューコメントの取得

```bash
# 未解決コメントのみ取得（--unresolved オプション時）
.claude/skills/gh-pr-review/scripts/get-review-comments.sh {pr_number} --unresolved

# 全コメント取得
.claude/skills/gh-pr-review/scripts/get-review-comments.sh {pr_number}
```

---

## Step 3: コメントの優先度分類

取得したコメントを以下の優先度で分類:

### 🔴 Critical（必須対応）

- バグ指摘
- セキュリティ問題
- ビルド/テスト失敗の原因
- "must", "required", "blocking" などのキーワード

### 🟡 Warning（対応推奨）

- コード品質の問題
- パフォーマンス懸念
- "should", "consider", "recommend" などのキーワード

### 🟢 Suggestion（任意対応）

- スタイル提案
- リファクタリング案
- "nit", "optional", "nice to have" などのキーワード

### ℹ️ Question（回答のみ）

- 設計意図の質問
- 確認事項
- "?" を含む、"why", "how" で始まる

---

## Step 4: 対応の実施

### 優先順位: Critical → Warning → Suggestion → Question

### 4.1 コード修正が必要な場合

1. 対象ファイルを読み取り（Read）
2. 関連コードを調査（Grep, Glob, Task で Explore）
3. 外部情報が必要なら調査（WebSearch, WebFetch）
4. 修正を実施（Edit）
5. ビルド/テスト確認

```bash
cargo check && cargo test
```

### 4.2 修正をコミット

レビュー対応であることを明記:

```bash
git add {修正ファイル}
git commit -m "$(cat <<'EOF'
fix: address review feedback

- [Critical] {対応内容}
- [Warning] {対応内容}

Co-Authored-By: Claude Sonnet <noreply@anthropic.com>
EOF
)"
```

### 4.3 プッシュ

```bash
git push
```

---

## Step 5: レビューコメントへの返信

### コメントへの返信

```bash
.claude/skills/gh-pr-review/scripts/reply-to-comment.sh \
  {pr_number} {comment_id} "{返信内容}"
```

### スレッドを解決済みにする

```bash
.claude/skills/gh-pr-review/scripts/resolve-thread.sh {pr_number} {thread_node_id}
```

### 一般コメントとして返信（スレッドに属さない場合）

```bash
gh pr comment {pr_number} --body "{返信内容}"
```

### 返信テンプレート

**修正完了時:**

```text
Fixed in {commit_hash}.
```

**対応不要と判断した場合:**

```text
Thank you for the suggestion.
I've decided to keep the current approach because {理由}.
```

**質問への回答:**

```text
{回答内容}
```

---

## Step 6: CI確認

```bash
gh pr checks {pr_number}
```

失敗している場合は原因を調査し、追加修正を行う。

---

## Step 7: 完了報告

```text
✅ レビュー対応が完了しました

PR: {url}
コミット: {hash}

対応サマリー:
- 🔴 Critical: {n}件 対応済み
- 🟡 Warning: {n}件 対応済み
- 🟢 Suggestion: {n}件 対応済み/{m}件 見送り
- ℹ️ Question: {n}件 回答済み

次のステップ:
- 追加のレビューを待つ
- マージ可能な場合 → /gh-pr-merge
- 状態を確認する場合 → /git-info
```

---

## 注意事項

- Force push は避け、追加コミットで対応（レビュー履歴を保持）
- Critical は必ず対応、Suggestion は見送り可（理由を返信）
- レビュアーの意図が不明な場合は、修正前に確認コメントを投稿
- 複数の Critical/Warning は論理的にまとめて1コミットにしても可

---

## 参考

- [gh pr-review 拡張](https://github.com/agynio/gh-pr-review) - AIエージェント向けPRレビューツール
- [GitHub CLI マニュアル](https://cli.github.com/manual/gh_pr_review)
