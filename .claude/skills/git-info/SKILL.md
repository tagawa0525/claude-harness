---
model: haiku
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git remote*)
  - Bash(git worktree*)
  - Bash(git stash*)
  - Bash(gh pr*)
  - Bash(gh auth*)
---

# Git Info Command

現在のGit状態を俯瞰する。

## 情報収集

### ブランチ情報

!`git branch -vv`

### 未コミット変更

!`git status --short`

### 未プッシュコミット

```bash
git log @{upstream}..HEAD --oneline 2>/dev/null || echo "(上流ブランチ未設定)"
```

### stash一覧

```bash
git stash list
```

### worktree一覧

```bash
git worktree list
```

### 関連PR状態

```bash
gh pr status 2>/dev/null || echo "(GitHub CLI未認証)"
```

## 出力フォーマット

```text
📍 現在のブランチ: [branch-name]
   上流: [origin/branch-name] [ahead/behind情報]

📝 未コミット変更:
   [git status --short の出力、またはなし]

📤 未プッシュコミット:
   [コミット一覧、またはなし]

📦 stash:
   [stash一覧、またはなし]

🌳 worktree:
   [worktree一覧]

🔗 関連PR:
   [PR情報、またはなし]
```

## 不要なworktreeの検出

マージ済みブランチのworktreeがある場合:

```text
⚠️ マージ済みブランチのworktreeがあります:

- ../myapp-feat-login (feat/login) - マージ済み

削除するには:
  /git-worktree feat/login --remove
```

## 推奨アクション

状態に応じて次のアクションを提案:

- 未コミット変更がある → `/git-commit`
- 未プッシュコミットがある → `/git-push`
- PRがない → `/git-pull-request`
- mainブランチで作業中 → `/git-branch` または `/git-worktree`
- 不要なworktreeがある → `/git-worktree --remove`

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合（ブランチ名など）は通常のテキスト質問
