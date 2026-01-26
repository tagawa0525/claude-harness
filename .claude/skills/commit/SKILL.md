---
model: haiku
argument-hint: [message]
allowed-tools:
  - Bash(git status*)
  - Bash(git diff*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git add*)
  - Bash(git commit*)
---

# Commit Command

ステージされた変更に対してConventional Commits形式のコミットメッセージを作成しコミットを実行。

## 現在の状態

!`git status --short`
!`git diff --cached --stat`
!`git log -5 --oneline`

## コミットメッセージ作成

$ARGUMENTS が指定されている場合はそれをコミットメッセージのベースとして使用。
指定がない場合はステージされた変更を分析し、以下の形式でメッセージを自動生成。

### Conventional Commits 形式

- **Type**: feat, fix, docs, style, refactor, test, chore
- **Subject**: 50文字以内、命令形、先頭小文字、末尾ピリオドなし

## コミット実行

```bash
git commit -m "$(cat <<'EOF'
[type]: [subject]

[optional body]

EOF
)"
```

## 失敗時の対応

コミットが失敗した場合（pre-commit hook エラーなど）:

1. エラー内容を表示
2. 選択肢を提示:
   - **自動修正**: フォーマッタやリンタの `--fix` オプションを実行
   - **手動修正**: ユーザーに修正を任せる
   - **中断**: コミットを中止

自動修正後は変更を再ステージングし、コミットを再試行。

## 完了確認

```bash
git status
```
