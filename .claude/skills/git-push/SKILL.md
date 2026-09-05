---
name: git-push
description: ローカルコミットをリモートにプッシュ。main/masterへの直接プッシュは禁止。
model: haiku
argument-hint: [--force]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git push*)
---

# Git Push Command

ローカルコミットをリモートリポジトリにプッシュする。

## 現在の状態

!`git status --short`
!`git branch -vv`

## hook が守るルール

以下は `guard-git-push` hook が機械的に deny する。手順で判断することはない:

- main/master への push、force push（`--force` / `-f` / `+refspec`）、`--all` / `--mirror`

feature branch への `--force-with-lease` は open PR があっても許可される
（マージ前に origin/main へリベースして push し直す運用のため）。

deny されたら、そのまま従う。どうしても必要な場合はユーザーの明示的な指示のもと、コマンドに
`ALLOW_PROTECTED_PUSH=1` を付けて実行する（勝手に付けない）。

現在のブランチが `main` / `master` なら push せず、PRワークフローを案内する:

```text
⚠️ main/master ブランチへの直接プッシュは推奨されません。

PRワークフローを使用してください:
1. /git-cherry-pick でブランチを分離
2. /git-push で feature ブランチをプッシュ
3. /gh-pr-create でプルリクエストを作成
```

## プッシュ対象の確認

```bash
git log @{upstream}..HEAD --oneline 2>/dev/null || echo "上流ブランチ未設定（push で自動設定される）"
```

## プッシュ実行

上流ブランチの有無は気にしなくてよい（`push.autoSetupRemote` により初回 push で自動設定される）:

```bash
git push
```

### --force オプションが指定された場合

`--force` ではなく `--force-with-lease` を使う（他の人がプッシュした変更を誤って上書きしない）:

```bash
git push --force-with-lease
```

open PR のあるブランチでは hook が拒否する。履歴を整理したい場合はマージ後に行うか、PRを閉じてから行う。

## 完了確認

```bash
git branch -vv
git log --oneline -3
```

## 次のステップ

```text
✅ プッシュしました。

次のステップ:
- PRを作成する場合 → /gh-pr-create
- 状態を確認する場合 → /git-info
```
