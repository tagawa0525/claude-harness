---
model: sonnet
argument-hint: <commit> [--to <branch>]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git show*)
  - Bash(git switch*)
  - Bash(git checkout*)
  - Bash(git cherry-pick*)
  - Bash(git rebase*)
  - Bash(git reset*)
  - Bash(git stash*)
---

# Git Cherry-Pick Command

ブランチ間でコミットを移動する（元ブランチからの除去を含む）。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git log --oneline -15`

## 使用シナリオ

### シナリオ1: mainで作業してしまった → featureブランチに移動

```text
Before:
main ─── A(feat) ─── B(feat)

After:
main (reset)
feat/xxx ─── A' ─── B'
```

### シナリオ2: featureブランチで複数目的が混在 → 別ブランチに分離

```text
Before:
feat/login ─── A(login) ─── B(fix) ─── C(login)

After:
feat/login ─── A ─── C'
fix/xxx ─── B'
```

## 処理の流れ

### Step 1: 移動するコミットを特定

$ARGUMENTS でコミットハッシュが指定されている場合はそれを使用。
指定がない場合はコミット履歴を分析し、移動候補を提案。

### Step 2: 移動先ブランチを決定

`--to` オプションで指定されている場合はそれを使用。
指定がない場合:

- 新規ブランチを作成するか確認
- ブランチ名の候補を提案（コミット内容に基づく）

### Step 3: cherry-pick 実行

```bash
# 移動先ブランチに切り替え（または作成）
git switch -c [new-branch] main  # 新規の場合
git switch [existing-branch]      # 既存の場合

# コミットを適用
git cherry-pick [commit-hash]
```

### Step 4: 元ブランチからコミットを除去

```bash
# 元ブランチに戻る
git switch [source-branch]

# rebase -i で対象コミットを drop
git rebase -i [base-commit]
```

## 注意事項

### リモートにプッシュ済みの場合

```text
⚠️ リモートにプッシュ済みのコミットを移動します。

以下の操作が必要になります:
1. 元ブランチ: force push（履歴が変わるため）
2. 新ブランチ: 通常のpush

続行しますか？
```

### コンフリクトが発生した場合

1. コンフリクトの内容を表示
2. 解決方法を提案
3. `git cherry-pick --continue` または `--abort` を案内

## 完了確認

```bash
git log --oneline -5 [source-branch]
git log --oneline -5 [target-branch]
```

## 関連コマンド

- `/git-tidy` - 同一ブランチ内のコミット整理
- `/git-branch` - 新規ブランチ作成
