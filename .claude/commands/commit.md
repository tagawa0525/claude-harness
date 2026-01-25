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

Git の pre-commit hook が品質チェックを実行するため、このコマンドはコミットメッセージの作成に特化します。

## 手順

1. 状態を確認:

```bash
git status
git diff --cached --stat
git log -5 --oneline
```

2. ステージされた変更を分析し、conventional commits 形式でコミットメッセージを作成

3. コミットを実行:

```bash
git commit -m "$(cat <<'EOF'
[type]: [subject]

[optional body]

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

4. 結果を確認:

```bash
git status
```

## Commit Message Guidelines

- **Type**: feat, fix, docs, style, refactor, test, chore
- **Subject**: 50文字以内、命令形、先頭大文字、末尾にピリオドなし
- **Body** (オプション): 変更の理由や詳細

## Notes

- pre-commit hook が失敗した場合、コミットは自動的に中断される
- `--no-verify` は使用しない（品質チェックをバイパスしてしまうため）
