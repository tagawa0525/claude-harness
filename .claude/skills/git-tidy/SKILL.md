---
model: haiku
argument-hint: [squash|split|reorder]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git show*)
  - Bash(git rebase*)
  - Bash(git reset*)
  - Bash(git add*)
  - Bash(git commit*)
  - Bash(git stash*)
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

1. 対象コミット数を確認
2. `git rebase -i HEAD~N`
3. 2番目以降を `squash` または `fixup` に変更
4. コミットメッセージを編集

### split - 1コミットを複数に分割

```text
Before:
└── "feat + fix 混在"

After:
├── "feat: 新機能"
└── "fix: バグ修正"
```

手順:

1. `git rebase -i HEAD~N` → 対象コミットを `edit` に変更
2. `git reset HEAD^` → 変更をステージング解除
3. 目的別にファイルを `git add` してコミット
4. `git rebase --continue`

### reorder - コミット順序の入れ替え

`git rebase -i` でコミット行の順序を入れ替え。

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

- 失敗時は `git rebase --abort` で元に戻せる

## 関連コマンド

- `/git-cherry-pick` - ブランチ間のコミット移動
