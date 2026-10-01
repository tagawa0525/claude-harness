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
---

# Git Tidy Command

同一ブランチ内のコミットを整理する（squash、split、reorder）。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git log --oneline -10`

## 前提

- `main` / `master` では実行しない（/git-branch で feature ブランチを作るよう案内する）
- 未コミットの変更があれば `git stash` で退避し、終わったら復元を提案する
- 書き換え前に元の HEAD のハッシュを記録する（`git reset --hard <元のHEAD>` で戻せるように）
- 履歴を分析して操作を提案し、ユーザーの確認を得てから実行する

## 操作

`git rebase -i` は対話的で使えないので、reset と cherry-pick で組み立てる:

- **squash**: `git reset --soft HEAD~N` → 新しいメッセージで `git commit`
- **split**: `git reset HEAD~1` → 目的別に `git add` してコミットを繰り返す。対象が直前の
  コミットでなければ、先に reorder で末尾へ移す
- **reorder**: 分岐点以降の**全**コミットを `git log --reverse --format=%h "$(git merge-base HEAD main)"..HEAD`
  で記録し、新しい順序を決める → `git reset --hard "$(git merge-base HEAD main)"` →
  記録した全コミットを 1 回ずつ新しい順序で `git cherry-pick`（並べ替えない分も含める。漏らすと消える）

push 済みのコミットを書き換えた場合は `/git-push --force`（`--force-with-lease`）が要ることを伝える。
