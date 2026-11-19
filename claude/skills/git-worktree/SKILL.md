---
name: git-worktree
description: 並行作業用に別ディレクトリで作業環境を作成。
model: haiku
argument-hint: <branch-name> [--remove]
allowed-tools:
  - Bash(git worktree*)
  - Bash(git branch*)
  - Bash(~/.claude/scripts/worktree-add.sh*)
---

# Git Worktree Command

## 現在の状態

!`git worktree list`
!`git branch -vv`

## 作成

$ARGUMENTS のブランチ名で作成する:

```bash
~/.claude/scripts/worktree-add.sh <branch-name>
```

- ブランチがローカルかリモートにあればそれをチェックアウトし、なければ既定ブランチ（`BASE:` の値）から
  新規作成する。今のブランチの続きにするときだけ `--from HEAD` を付ける
- `SHARED:` 行は、`.worktreelink` に書いたパスをメイン worktree への symlink にしたことを示す
- `NOTE:` 行と `ERROR:` 行は指示に従う

### `ERROR:` の修正方法

`.worktreelink` の誤りの `ERROR:` は、出力の直す順と、各パスを張るコマンドに従う（スクリプトは再実行しない）。

## 削除（--remove 指定時）

ブランチ名で指定された場合は `git worktree list` からパスを特定する。

```bash
git worktree remove <path>
git worktree prune
```

## 完了メッセージ

### 作成時

```text
✅ worktree を作成しました。

場所: <WORKTREE の値>
ブランチ: <branch-name>

作業を開始するには:
  cd <WORKTREE の値>
```

### 削除時

```text
✅ worktree を削除しました: <path>

現在の worktree:
<git worktree list の出力>
```
