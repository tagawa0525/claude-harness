---
name: git-branch
description: featureブランチの作成・リネーム。変更内容から適切なブランチ名を自動生成。
model: opus
allowed-tools:
  - Bash(git *)
  - Read
  - Glob
  - AskUserQuestion
---

# Git Branch Command

変更内容から適切なブランチ名を自動生成し、ブランチの作成またはリネームを行う。

## 現在の状態

!`git branch -vv`
!`git status --short`
!`git log --oneline -5`
!`git fetch -q; git log --oneline HEAD..origin/main 2>/dev/null | head -3`

## 動作モードの判定

現在のブランチによって動作が変わる:

- **mainブランチ** → 新規featureブランチを作成
- **featureブランチ** → 現在のブランチをリネーム

## ブランチ名の自動生成

**常にLLMがブランチ名を生成する。** 引数は受け付けない。

### Step 1: 変更内容の分析

以下を確認してブランチ名を決定:

- mainブランチの場合:
  1. ステージされた変更: `git diff --cached --name-only`
  2. 未ステージの変更: `git diff --name-only`
  3. 変更ファイルの内容を読んで意図を把握
- featureブランチの場合:
  1. mainとの差分: `git log --oneline main..HEAD`
  2. 未コミットの変更: `git diff --name-only`
  3. コミットメッセージとファイルの内容から全体の意図を把握

### Step 2: 命名規則に従い候補を生成

- `feat/xxx` - 新機能
- `fix/xxx` - バグ修正
- `refactor/xxx` - リファクタリング
- `docs/xxx` - ドキュメント
- `chore/xxx` - その他

### Step 3: AskUserQuestion で候補を提示

2-3個の候補を生成し、ユーザーに選択させる。
「Other」で自由入力も可能。

## mainブランチの場合: ブランチ作成

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

## featureブランチの場合: リネーム

### リモート追跡の有無で分岐

```bash
# リモートにpush済みかチェック
git ls-remote --heads origin [current-branch]
```

**リモートにpush済みの場合:**

```text
⚠️ リモートブランチが存在します。リネームするとリモートも更新されます。

続行しますか？
```

承認後:

```bash
git branch -m [old-name] [new-name]
git push origin :[old-name] [new-name]
git push -u origin [new-name]
```

**ローカルのみの場合:**

```bash
git branch -m [old-name] [new-name]
```

## 完了確認

```bash
git branch -vv
```

```text
✅ ブランチ「[branch-name]」を[作成|リネーム]しました。

次のステップ:
- コードを編集 → /git-commit
- プッシュ → /git-push
- PR作成 → /gh-pr-create
```
