---
name: git-branch
description: featureブランチの作成・リネーム。
# 変更内容の分析とブランチ名生成の品質を優先
model: sonnet
allowed-tools:
  - Bash(git *)
  - Bash(~/.claude/scripts/rename-plan.sh*)
  - Bash(~/.claude/scripts/rename-branch.sh*)
  - Read
  - Glob
---

# Git Branch Command

変更内容からブランチ名を生成し、main / master にいれば feature ブランチを作成、feature ブランチにいれば
そのブランチをリネームする。引数は受け付けない（名前は常に生成する）。

## 現在の状態

!`git branch -vv`
!`git status --short`
!`git log --oneline -5`
!`git fetch -q; git log --oneline HEAD..origin/main 2>/dev/null | head -3`
!`~/.claude/scripts/rename-plan.sh --list`

## ブランチ名の自動生成

### Step 1: 計画書の把握

上の `rename-plan.sh --list` の出力（連番のない計画書）があれば読み、タイトル（`# ...` 行）から意図を
把握する。計画書はブランチ名の最優先の材料にする。

### Step 2: 変更内容の分析

計画書がないときは、変更から意図を把握する:

- mainブランチ: `git diff --cached --name-only` と `git diff --name-only` のファイルの内容
- featureブランチ: `git log --oneline main..HEAD` のコミットメッセージと、`git diff --name-only` のファイルの内容

### Step 3: 命名規則に従い名前を決める

- `feat/xxx` - 新機能
- `fix/xxx` - バグ修正
- `refactor/xxx` - リファクタリング
- `docs/xxx` - ドキュメント
- `chore/xxx` - その他

名前は 1 つに決めてそのまま使い、完了確認で伝える。

## mainブランチの場合: ブランチ作成

### 起点の確認

上の `HEAD..origin/main` のログに出力があれば（ローカルmainが遅れている）、origin/main から分岐し、完了確認でそう伝える
（ローカルの main は動かさない）。

### ブランチ作成コマンド

```bash
# 通常（ローカルHEADから分岐）
git switch -c [branch-name]

# origin/mainから分岐する場合
git switch --no-track -c [branch-name] origin/main

# 既存のブランチ名を origin/main から作り直す場合（そのブランチのコミットは外れる）
git switch -C [branch-name] --no-track origin/main
git branch --unset-upstream [branch-name]
```

作り直す場合:

- 先に `git worktree list` でそのブランチが別の worktree で使われていないことを確かめ、
  `git rev-parse [branch-name]` で元の位置を控える
- `--unset-upstream` は `&&` でつながずに実行する。一度も push していないブランチでは
  `has no upstream information`（終了コード 128）で失敗するが、外す上流が無いだけなのでそのまま進める

## featureブランチの場合: リネーム

```bash
~/.claude/scripts/rename-branch.sh [new-name]
```

- exit 2（`STOP:` 行）: リモートに同名ブランチがある。リモートも更新されることをユーザーに伝え、
  承認を得てから `--remote` を付けて再実行する

## 計画書のリネーム

ブランチ作成・リネーム後、連番のない計画書があれば連番つきにリネームする（コミットまでスクリプトが行う）:

```bash
~/.claude/scripts/rename-plan.sh [branch-name]
```

## 完了確認

```bash
git branch -vv
```

```text
✅ ブランチ「[branch-name]」を[作成|リネーム]しました。
[計画書リネーム時: 📋 計画書を [NNN]_[name].md にリネームしました。]

次のステップ:
- コードを編集 → /git-commit
- プッシュ → /git-push
- PR作成 → /gh-pr-create
```
