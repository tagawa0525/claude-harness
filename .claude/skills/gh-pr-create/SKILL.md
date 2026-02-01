---
model: haiku
argument-hint: [--draft] [--reviewer REVIEWER]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git push*)
  - Bash(gh pr*)
  - Bash(gh auth*)
---

# GitHub PR Create Command

GitHub Pull Requestを作成する（gh CLI使用）。

## 事前確認

!`gh auth status`
!`git status --short`
!`git branch -vv`

## 未プッシュコミットの確認

```bash
git log @{upstream}..HEAD --oneline 2>/dev/null
```

未プッシュコミットがある場合は先にプッシュ:

```bash
git push -u origin $(git branch --show-current)
```

## PR内容の生成

ベースブランチとの差分を分析:

```bash
git log main..HEAD --oneline
git diff main..HEAD --stat
```

### PRタイトル

- 最初のコミットメッセージまたは変更の要約から生成
- 70文字以内

### PR本文テンプレート

```markdown
## 概要
[変更内容の要約]

## 変更点
- [主要な変更点をリスト]

## テスト
- [ ] 動作確認済み
- [ ] テスト追加/更新済み
```

## PR作成

### 通常のPR

```bash
gh pr create --title "[タイトル]" --body "$(cat <<'EOF'
## 概要
[要約]

## 変更点
- [変更点]

## テスト
- [ ] 動作確認済み
EOF
)"
```

### ドラフトPR（--draft 指定時）

```bash
gh pr create --draft --title "[タイトル]" --body "[本文]"
```

### レビュアー指定（--reviewer 指定時）

```bash
gh pr create --reviewer [REVIEWER] --title "[タイトル]" --body "[本文]"
```

## 完了

PR作成後、URLを表示:

```bash
gh pr view --web
```

## 次のステップ

```text
✅ PRを作成しました: [URL]

次のステップ:
- レビュー後にマージする場合 → /gh-pr-merge
- 状態を確認する場合 → /git-info
```

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合（ブランチ名など）は通常のテキスト質問
