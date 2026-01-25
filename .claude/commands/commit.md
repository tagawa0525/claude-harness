---
model: haiku
allowed-tools:
  - Bash(git status*)
  - Bash(git diff*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git add*)
  - Bash(git commit*)
---

# Commit Command

ステージされた変更に対して適切なコミットメッセージを作成し、コミットを実行する。

## 1. 状態確認

```bash
git status
git diff --cached --stat
git log -5 --oneline
```

## 2. コミットメッセージ作成

ステージされた変更を分析し、conventional commits 形式でメッセージを作成:

- **Type**: feat, fix, docs, style, refactor, test, chore
- **Subject**: 50文字以内、命令形、先頭大文字、末尾ピリオドなし
- **Body**: 必要に応じて変更の理由や詳細を記載

## 3. コミット実行

```bash
git commit -m "$(cat <<'EOF'
[type]: [subject]

[optional body]

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

## 4. 失敗時の対応

コミットが失敗した場合（pre-commit hook エラーなど）:

1. エラー内容をユーザーに表示
2. 以下の選択肢を提示:
   - **自動修正**: `nixpkgs-fmt`, `ruff format --fix`, `markdownlint --fix` 等を実行
   - **手動修正**: ユーザーに修正を任せる
   - **中断**: コミットを中止

ユーザーの選択に従って対応する。自動修正後は変更を再ステージングし、コミットを再試行する。

## 5. 完了確認

```bash
git status
```
