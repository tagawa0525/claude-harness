---
name: git-worktree
description: 並行作業用に別ディレクトリで作業環境を作成。複数PRの同時進行に最適。
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
- fetch の失敗で `ERROR:` になったら（オフライン）、ローカルにあるブランチか `--from` を使う
- `SHARED:` 行は、`.worktreelink` に書いたパスをメイン worktree への symlink にしたことを示す
- `NOTE:` 行は指示に従う。メイン worktree に無いパスの `NOTE:` は、必要ならメイン worktree に用意してから
  `ln -s` で足す

### `ERROR:` の修正方法

`.worktreelink` の誤りで `ERROR:` になったとき、worktree は残っていて、symlink は 1 つも作られていない。
作成先があるのでスクリプトは再実行できない。

1. その worktree（そのブランチ）で `.worktreelink` か `.gitignore` を直す（書き方は `worktree-add.sh` の冒頭のコメント）
2. `.worktreelink` の各パスを張る（先は絶対パス）:

```bash
mkdir -p "$(dirname <worktree>/<path>)"
ln -s <メイン worktree>/<path> <worktree>/<path>
```

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
