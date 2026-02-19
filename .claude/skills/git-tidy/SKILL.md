---
name: git-tidy
description: 同一ブランチ内のコミットを整理。squash、分割、並べ替えに対応。
model: sonnet
argument-hint: [squash|split|reorder]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git show*)
  - Bash(git reset*)
  - Bash(git add*)
  - Bash(git commit*)
  - Bash(git stash*)
  - Bash(git cherry-pick*)
  - Bash(git merge-base*)
---

# Git Tidy Command

同一ブランチ内のコミットを整理する（squash、分割、reorder）。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git log --oneline -10`

## mainブランチでの実行禁止

現在のブランチが `main` または `master` の場合:

```text
⚠️ mainブランチでは履歴の書き換えは推奨されません。

featureブランチで作業してください:
  /git-branch feat/xxx
```

## 操作モード

### squash - 複数コミットを1つにまとめる

```text
Before:
├── "WIP"
├── "typo fix"
└── "完成"

After:
└── "feat: ログイン機能追加"
```

手順:

1. 対象コミット数 N を確認
2. `git reset --soft HEAD~N` で変更をステージに戻す
3. `git commit` で新しいメッセージでコミット

```bash
# 例: 直近3コミットを1つにまとめる
git reset --soft HEAD~3
git commit -m "feat: ログイン機能追加"
```

### split - 1コミットを複数に分割

```text
Before:
└── "feat + fix 混在"

After:
├── "feat: 新機能"
└── "fix: バグ修正"
```

手順:

1. `git reset HEAD~1` で直前コミットの変更をワーキングツリーに戻す
2. 目的別にファイルを `git add` してコミット
3. 残りの変更も `git add` してコミット

```bash
# 例: 直前コミットを分割
git reset HEAD~1
git add src/feature.rs
git commit -m "feat: 新機能"
git add src/bugfix.rs
git commit -m "fix: バグ修正"
```

注: 分割対象が直前コミットでない場合は、先にreorderで直前に移動してから分割する。

### reorder - コミット順序の入れ替え

分岐点まで `git reset --hard` し、`git cherry-pick` で希望順に再配置。

手順:

1. 分岐点を特定: `git merge-base HEAD main`
2. 並べ替えたいコミットハッシュを記録
3. 分岐点まで `git reset --hard [merge-base]`
4. `git cherry-pick` で希望順にコミットを再適用

```bash
# 例: コミット A, B, C を C, A, B の順に並べ替え
BASE=$(git merge-base HEAD main)
git reset --hard "$BASE"
git cherry-pick [C-hash] [A-hash] [B-hash]
```

⚠️ `git reset --hard` 前に、対象コミットハッシュを必ず記録すること。

## 処理フロー

1. 未コミット変更がある場合は `git stash` で退避
2. コミット履歴を分析
3. 適切な操作を提案
4. ユーザー確認後に実行
5. stash があれば復元を提案

## 注意事項

- リモートにプッシュ済みの場合は force push が必要:

  ```text
  ⚠️ リモートにプッシュ済みのコミットを変更します。
  /git-push --force でforce pushが必要になります。
  ```

- 操作前にコミットハッシュを記録しておけば、`git reset --hard [元のHEAD]` で復元可能

## 関連コマンド

- `/git-cherry-pick` - ブランチ間のコミット移動

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合（ブランチ名など）は通常のテキスト質問
