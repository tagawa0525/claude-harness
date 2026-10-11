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

並行作業用に別ディレクトリで作業環境を作成する。

## 現在の状態

!`git worktree list`
!`git branch -vv`

## 使用シナリオ

- 複数のPRを同時に進める
- レビュー待ちの間に別作業
- 長時間かかる作業を中断せず別タスク

## 作成

$ARGUMENTS のブランチ名で作成する:

```bash
~/.claude/scripts/worktree-add.sh <branch-name>
```

スクリプトが決めること（手順で考えなくてよい）:

- 作成先は親ディレクトリの `<repo>-<branch>`（ブランチ名の `/` は `-` に変換）
- ブランチがローカル/リモートに既にあればそれをチェックアウト、なければ既定ブランチ（`BASE:` の値）から新規作成。
  今のブランチの続きにするときだけ `--from HEAD` を付ける
- GitHub リモートが複数ある（fork 運用）という `NOTE:` 行が出たら、worktree で `gh pr` 系を使う前に
  `gh repo set-default --view` で解決先を確認する。未設定だと PR 番号が意図しない側の
  リポジトリで解決され「PRが見つからない」になる
- リポジトリの `.worktreelink` に書いたパスは、メイン worktree への symlink になる（`SHARED:` 行）。
  メイン worktree に無いパスは `NOTE:` 行で知らせるので、必要ならメイン worktree に用意してから
  `ln -s` で足す。`.worktreelink` の誤りで `ERROR:` になったら、worktree は残っているので、
  そのブランチで `.worktreelink` か `.gitignore` を直す（書き方は nixfiles の ADR-0016）。スクリプトは
  作成先があるので再実行できず、symlink は 1 つも作られていない。直した後、`.worktreelink` の各パスを
  `ln -s <メイン worktree>/<path> <worktree>/<path>`（先は絶対パス）で張る

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

## 注意事項

- 同じブランチを複数のworktreeでチェックアウトすることはできない
- worktreeのディレクトリを手動で削除した場合は `git worktree prune` が必要
- マージ済みブランチのworktreeは `/git-info` で検出される

## 関連コマンド

- `/git-branch` - 通常のブランチ切り替え（同じディレクトリ）
- `/git-info` - 不要なworktreeの検出
- `/gh-pr-merge` - マージ後にworktreeを自動削除
