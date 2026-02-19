---
name: git-commit
description: ステージされた変更をConventional Commits形式でコミット。メッセージ省略時は自動生成。
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

# Git Commit Command

ステージされた変更に対してConventional Commits形式のコミットメッセージを作成しコミットを実行。

## 現在の状態

!`git branch --show-current`
!`git status --short`
!`git diff --cached --stat`

## mainブランチでの作業警告

現在のブランチが `main` または `master` の場合:

```text
⚠️ mainブランチで作業中です。

作業方法を選択してください:
1. worktree作成        - 並行作業（別ディレクトリで作業）
2. featureブランチ作成  - 通常の機能開発（このディレクトリで切り替え）
3. mainのまま続行       - 小さな修正、設定変更など
```

この警告を表示し、ユーザーの選択を待つ。強制的にコミットを続行しない。

### featureブランチ/worktree選択時

ステージされた変更内容を分析し、適切なブランチ名の候補を2-3個生成して `AskUserQuestion` で提示。
ユーザーは候補から選択するか、「Other」でカスタム名を入力できる。

候補生成の例:

- 変更がスキル関連 → `feat/skills-xxx`, `refactor/skills-xxx`
- バグ修正 → `fix/xxx`
- ドキュメント → `docs/xxx`

## 大規模変更の警告

ステージされた変更が以下の条件を満たす場合は警告:

- ファイル数が5以上
- または変更行数が100行以上

```text
⚠️ 大規模な変更です（[N]ファイル、[M]行）

小さなコミットに分割することを推奨します:
- 関連する変更のみをステージング: git add [file]
- 部分的なステージング: git add -p

このまま続行しますか？
```

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
git log --oneline -1
```

## 次のステップ

コミット完了後:

```text
✅ コミットしました: [commit-hash] [message]

次のステップ:
- さらに変更を続ける場合 → 編集して /git-commit
- プッシュする場合 → /git-push
- コミットを整理する場合 → /git-tidy
```
