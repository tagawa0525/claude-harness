---
name: git-commit
description: ステージされた変更をコミット。
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

**GitHubリモートあり → worktree**（未コミットの変更も一緒に移る）:

```bash
~/.claude/scripts/worktree-add.sh [branch-name] --carry-changes
cd <出力の WORKTREE の値>
```

以降のステージング・コミット・プッシュはworktree側で実行する。

`WORKTREE:` を出したうえで `ERROR:` で終わったとき（`.worktreelink` の誤り）は、再実行せず、その worktree に移って
/git-worktree の「`ERROR:` の修正方法」に従う。

**GitHubリモートなし → その場でfeatureブランチ:**

```bash
git switch -c [branch-name]
```

## ステージング

何もステージされていない場合は、変更を論理単位に分けて `git add [file]` / `git add -p` し、
単位ごとにコミットする。

`.md` の lint 修正は git の pre-commit hook が自動で行い再ステージする。

## コミットメッセージ作成

$ARGUMENTS が指定されている場合はConventional Commits形式に整形して使用。
指定がない場合はステージされた変更を分析して生成。

- **Type**: feat, fix, docs, style, refactor, test, chore
- **Subject**: 50文字以内を目安、72文字が上限、命令形、先頭小文字、末尾ピリオドなし
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

pre-commit hookエラー等で失敗した場合、原因を直して再試行する。ファイルの中身の失敗は、フォーマッタ等の
`--fix` があればそれで、なければ手で直して再ステージし、メッセージの形式の失敗はメッセージを直す。
直すのはステージした変更が原因のものだけで、ほかの原因（もとからある違反など）は /topic-triage で振り分ける。
直せなければ、迂回せずにエラー内容と原因を報告して止まる。
Markdown で「unfixable issues remain」と出たら language-checks の markdown-checks.md を見て手で直す。

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
