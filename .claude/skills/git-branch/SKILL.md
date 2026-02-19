---
name: git-branch
description: featureブランチを作成して作業開始。mainブランチからのみ実行可能。
model: sonnet
allowed-tools:
  - Bash(git *)
  - Read
  - Glob
  - AskUserQuestion
---

# Git Branch Command

featureブランチを作成して作業を開始する。

## 現在の状態

!`git branch -vv`
!`git status --short`
!`git fetch -q; git log --oneline HEAD..origin/main 2>/dev/null | head -3`

## 動作条件

このコマンドは **mainまたはmasterブランチ** で実行された場合のみ動作する。

### 既にfeatureブランチにいる場合

```text
⚠️ 既にfeatureブランチ「[current-branch]」で作業中です。

新しいブランチを作成する場合は、先にmainに戻ってください:
  git switch main
```

## ブランチ名の自動生成

**常にLLMがブランチ名を生成する。** 引数は受け付けない。

### Step 1: 変更内容の分析

以下を確認してブランチ名を決定:

1. ステージされた変更: `git diff --cached --name-only`
2. 未ステージの変更: `git diff --name-only`
3. 変更ファイルの内容を読んで意図を把握

### Step 2: 命名規則に従い候補を生成

- `feat/xxx` - 新機能
- `fix/xxx` - バグ修正
- `refactor/xxx` - リファクタリング
- `docs/xxx` - ドキュメント
- `chore/xxx` - その他

### Step 3: AskUserQuestion で候補を提示

2-3個の候補を生成し、ユーザーに選択させる。
「Other」で自由入力も可能。

## ブランチ作成

### 起点の確認

ローカルmainが `origin/main` より遅れている場合（上記のログ出力がある場合）は警告:

```text
⚠️ ローカルmainがorigin/mainより遅れています。

1. pullしてから分岐 (推奨)
2. origin/mainから直接分岐
3. このまま続行
```

### ブランチ作成コマンド

```bash
# 通常（ローカルHEADから分岐）
git switch -c [branch-name]

# origin/mainから分岐する場合
git switch -c [branch-name] origin/main
```

## 完了確認

```bash
git branch -vv
```

作成後のメッセージ:

```text
✅ ブランチ「[branch-name]」を作成しました。

次のステップ:
1. コードを編集
2. /git-commit でコミット
3. /git-push でプッシュ
4. /gh-pr-create でPR作成
```
