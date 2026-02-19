---
name: git-push
description: ローカルコミットをリモートにプッシュ。main/masterへの直接プッシュは禁止。
model: haiku
argument-hint: [--force]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git remote*)
  - Bash(git rev-list*)
  - Bash(git push*)
---

# Git Push Command

ローカルコミットをリモートリポジトリにプッシュする。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git remote -v`

## main/master ブランチへの直接プッシュ禁止

現在のブランチが `main` または `master` の場合:

1. **プッシュを実行しない**
2. 以下のメッセージを表示:

```text
⚠️ main/master ブランチへの直接プッシュは推奨されません。

PRワークフローを使用してください:
1. /git-cherry-pick でブランチを分離
2. /git-push で feature ブランチをプッシュ
3. /gh-pr-create でプルリクエストを作成
```

## プッシュ対象の確認

```bash
git log @{upstream}..HEAD --oneline 2>/dev/null \
  || git log origin/$(git branch --show-current)..HEAD --oneline 2>/dev/null \
  || echo "上流ブランチ未設定"
```

## プッシュ実行

### 上流ブランチが未設定の場合

```bash
git push -u origin $(git branch --show-current)
```

### 上流ブランチが設定済みの場合

```bash
git push
```

### --force オプションが指定された場合

`--force-with-lease` の使用を推奨:

```bash
git push --force-with-lease
```

理由: 他の人がプッシュした変更を誤って上書きすることを防ぐ

## 完了確認

```bash
git branch -vv
git log --oneline -3
```

## 次のステップ

```text
✅ プッシュしました。

次のステップ:
- PRを作成する場合 → /gh-pr-create
- 状態を確認する場合 → /git-info
```
