---
name: git-info
description: 現在のGit状態を俯瞰表示。ブランチ、変更、コミット、PR、worktree、stashを一覧。
model: haiku
context: fork
allowed-tools:
  - Bash(~/.claude/scripts/git-info.sh)
---

# Git Info Command

現在のGit状態を俯瞰する。収集・整形・マージ済み worktree の検出はスクリプトが行う。

## 情報収集

!`~/.claude/scripts/git-info.sh`

## 推奨アクション

上の出力をそのまま提示し、状態に応じて次のアクションを提案する:

- 未コミット変更がある → `/git-commit`
- 未プッシュコミットがある → `/git-push`
- PRがない → `/gh-pr-create`
- mainブランチで作業中 → `/git-branch` または `/git-worktree`
- 「⚠️ マージ済みブランチのworktreeがあります」が出ている → 一覧にある `/git-worktree <branch> --remove`
