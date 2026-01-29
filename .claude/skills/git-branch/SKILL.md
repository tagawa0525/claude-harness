---
model: haiku
argument-hint: <branch-name>
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git switch*)
  - Bash(git checkout*)
---

# Git Branch Command

featureブランチを作成して作業を開始する。

## 現在の状態

!`git branch -vv`
!`git status --short`

## 動作条件

このコマンドは **mainまたはmasterブランチ** で実行された場合のみ動作する。

### 既にfeatureブランチにいる場合

```text
⚠️ 既にfeatureブランチ「[current-branch]」で作業中です。

新しいブランチを作成する場合は、先にmainに戻ってください:
  git switch main
```

## ブランチ名

$ARGUMENTS が指定されている場合はそれをブランチ名として使用。
指定がない場合はユーザーに確認。

### 命名規則

- `feat/xxx` - 新機能
- `fix/xxx` - バグ修正
- `refactor/xxx` - リファクタリング
- `docs/xxx` - ドキュメント
- `chore/xxx` - その他

## ブランチ作成

```bash
git switch -c [branch-name]
```

## 完了確認

```bash
git branch -vv
```

作成後のメッセージ:

```text
✅ ブランチ「[branch-name]」を作成しました。

次のステップ:
1. コードを編集
2. /git-commit でコミット
3. /git-push でプッシュ
4. /git-pull-request でPR作成
```

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合（ブランチ名など）は通常のテキスト質問
