---
name: git-commit
description: ステージされた変更をConventional Commits形式でコミット。メッセージ省略時は自動生成。
model: sonnet
argument-hint: [message]
allowed-tools:
  - Bash(git status*)
  - Bash(git diff*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git add*)
  - Bash(git commit*)
  - Bash(git remote*)
  - Bash(git switch*)
  - Bash(~/.claude/scripts/worktree-add.sh*)
  - Bash(cd*)
  - AskUserQuestion
---

# Git Commit Command

## 現在の状態

!`git branch --show-current`
!`git status --short`
!`git diff --cached --stat`

変更が全くない場合は中断する。

## mainブランチからの退避

現在のブランチが `main` / `master` の場合はhookでコミットできないため、先に退避する。
ブランチ名は変更内容から `feat/` `fix/` `refactor/` `docs/` `chore/` + kebab-case で生成する。

退避先はGitHubリモートの有無で分岐する:

```bash
git remote -v | grep -q 'github\.com'
```

**GitHubリモートあり → worktree**（未コミットの変更も一緒に移す）:

```bash
~/.claude/scripts/worktree-add.sh [branch-name] --carry-changes
cd <出力の WORKTREE の値>
```

以降のステージング・コミット・プッシュはworktree側で実行する。

**GitHubリモートなし → その場でfeatureブランチ:**

```bash
git switch -c [branch-name]
```

## ステージング

何もステージされていない場合は、変更を論理単位に分けて `git add [file]` / `git add -p` し、
単位ごとにコミットする。対象を絞らない `git add -A` / `git add .` は `guard-git-add.sh` hook が deny する。

ステージ済みの変更が大きい（5 ファイル以上または 100 行以上）と、`warn-large-commit.sh` hook が
コミット時に件数を知らせる。1 つの論理的変更に収まっているか確認し、複数の変更が混在していれば
`git add [file]` / `git add -p` で分けて別々にコミットする。大きくても 1 つの変更ならそのまま続行してよい。

コミット対象に機密情報（.env や秘密鍵のファイル、追加行のトークンや `password = "…"`）があると
`block-secret-commit.sh` hook が deny する。該当ファイルを `git restore --staged` で外して
`.gitignore` に追加し、push 済みの値はローテーションする。誤検出のときだけ、その行に
`gitleaks:allow` を書くかコマンドに `ALLOW_SECRET_COMMIT=1` を付ける。

## Markdown自動修正

`.md` の自動修正（`markdownlint --fix` と `fix-markdown-lint.py`）は git の pre-commit hook
（`modules/home/parts/git.nix`）がコミット時に行い、修正済みファイルを再ステージする。
手順としては何もしなくてよい。hook が「unfixable issues remain」で止めた場合だけ、
language-checks の markdown-checks.md を参照して手で直す。

## コミットメッセージ作成

$ARGUMENTS が指定されている場合はConventional Commits形式に整形して使用。
指定がない場合はステージされた変更を分析して生成。

- **Type**: feat, fix, docs, style, refactor, test, chore
- **Subject**: 50文字以内を目安（72文字超は commit-msg hook が拒否）、命令形、先頭小文字、末尾ピリオドなし
- **Body**: 理由がsubjectから自明でない場合のみ

## コミット実行

```bash
git commit -m "$(cat <<'EOF'
[type]: [subject]

[optional body]
EOF
)"
```

## 失敗時の対応

pre-commit hookエラー等で失敗した場合、エラー内容を表示し、
自動修正（フォーマッタ等の `--fix`）/ 手動修正 / 中断 を選択させる。
自動修正後は再ステージして再試行。

## 完了確認

```bash
git status
git log --oneline -1
```

## 次のステップ

```text
✅ コミットしました: [commit-hash] [message]

次のステップ:
- さらに変更を続ける場合 → 編集して /git-commit
- プッシュする場合 → /git-push
- コミットを整理する場合 → /git-tidy
```
