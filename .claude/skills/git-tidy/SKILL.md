---
name: git-tidy
description: 同一ブランチ内のコミットを整理。squash、分割、並べ替えに対応。
# model 未指定: 履歴書き換えの判断が必要なためセッションモデルを継承する
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
  - Bash(git fetch*)
---

# Git Tidy Command

同一ブランチ内のコミットを整理する（squash、split、reorder）。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git log --oneline -10`

## 前提

- `main` / `master` では実行しない（/git-cherry-pick で feature ブランチへ移すよう案内する）
- 未コミットの変更があれば `git stash` で退避し、終わったら復元を提案する
- 書き換え前に `git branch [backup] HEAD` で元の HEAD を残し、結果を確認してから消す
- 履歴を分析して操作を提案し、ユーザーの確認を得てから実行する。push 済みなら
  `/git-push --force` が要ることも確認の時点で伝える

## 操作

`git rebase -i` は対話的で使えないので、reset と cherry-pick で組み立てる:

- **squash**: `git reset --soft HEAD~N` → 新しいメッセージで `git commit`
- **split**: `git reset HEAD~1` → 目的別に `git add` してコミットを繰り返す。対象が直前の
  コミットでなければ、先に reorder で末尾へ移す
- **reorder**: 分岐点以降の**全**コミットを希望順に cherry-pick し直す（並べ替えないものも含める。
  漏らすと消える）:

```bash
git fetch origin
BASE=$(git merge-base HEAD origin/main)   # ローカル main は古いことがある
git log --reverse --format=%h "$BASE"..HEAD
git reset --hard "$BASE"
git cherry-pick [全ハッシュを希望順で]
```
