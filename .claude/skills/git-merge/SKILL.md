---
name: git-merge
description: featureブランチをローカルでmainにマージ。GitHubを経由しないリポジトリ用。チェック・レビュー・rebase・マージコミット・後片付けまで行う。
# model 未指定: レビュー指摘への対応、コンフリクト解決、マージコミットメッセージの判断が必要なためセッションモデルを継承する
argument-hint: [branch]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git show*)
  - Bash(git switch*)
  - Bash(git rebase*)
  - Bash(git merge*)
  - Bash(git rev-list*)
  - Bash(git rev-parse*)
  - Bash(git worktree*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh*)
---

# Git Merge Command

GitHub リモートを持たないリポジトリで、feature ブランチをローカルの main にマージする。
GitHub での PR の流れ（CI → 自動レビュー → `gh pr merge --merge`）を、ローカルの手順に置き換えたもの。

## 現在の状態

!`git status --short`
!`git branch -vv`
!`git worktree list`
!`git log --oneline --graph --all -20`

## 対象

$ARGUMENTS のブランチ（省略時は現在のブランチ）をマージする。

- `main` / `master` 自身は対象にしない
- 未コミットの変更があれば中断し、/git-commit を案内する
- GitHub リモートがあるリポジトリでは使わず、/gh-pr-create と /gh-pr-merge を案内する

## main がまだない場合（最初のマージ）

リポジトリの最初のコミットが feature ブランチ上にあって main が存在しないときは、
ルートコミット（プロジェクトの初期化コミット）を指す main を作る:

```bash
git rev-list --max-parents=0 [branch]   # ルートコミットが1つで、初期化だけの内容であることを確認する
git branch main [root-commit]
```

ルートコミットに初期化以外の変更が含まれていれば、作らずにユーザーに確認する。

## ブランチの積み重なりの確認

対象ブランチが、まだマージしていない別の feature ブランチの上に積まれていることがある
（`git log --oneline main..[branch]` に他のブランチのコミットが含まれる）。
その場合は土台のブランチから順にこの手順でマージし、対象ブランチは後で rebase する。

## rebase

main が対象ブランチの分岐点より進んでいれば、対象ブランチを main の上に rebase して
git graph を整える（GitHub での「head の遅れ」の解消に相当）:

```bash
git switch [branch]
git rebase main
```

コンフリクトしたら内容と解決案を示し、解決して `git rebase --continue` するか
`git rebase --abort` するかを決める。

## チェック（CI の代わり）

対象ブランチで品質チェックを実行する:

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh
```

失敗したら原因を直してコミットし、もう一度チェックする。通らないままマージしない。

## レビュー（自動レビューの代わり）

`/code-review` で main との差分をレビューし、指摘に対応してからマージする:

- 指摘を修正したら、コミットしてチェックからやり直す
- 対応しない指摘は、その理由をユーザーに伝える

## マージコミットメッセージ

ブランチの内容を把握してから書く:

```bash
git log --format='%h %s%n%b' main..[branch]
git diff --stat main...[branch]
```

```text
Merge: [ブランチの変更を簡潔に要約]

## Why
[目的・背景を1-2文]

## What
- [主要な変更点を3-5項目]

## Impact
[影響するモジュール/機能]

Branch: [branch]
```

## マージ実行

fast-forward せず、必ずマージコミットを作る（GitHub での `--merge` と同じ履歴になる）:

```bash
git switch main
git merge --no-ff [branch] -m "$(cat <<'EOF'
[マージコミットメッセージ]
EOF
)"
```

`git merge` は `-F -`（標準入力からの読み込み）に対応していないので、`-m` に渡す。

## 後片付け

```bash
git worktree list                 # 対象ブランチの worktree があれば
git worktree remove [path]        # 変更が残っていれば失敗するので、確認して対処する
git branch -d [branch]            # -d なので、未マージなら失敗する
```

## 完了確認

```bash
git log --oneline --graph -10
```

```text
✅ [branch] を main にマージしました: [merge-commit-hash]

後片付け:
- worktree [path] を削除（該当する場合）
- ブランチ [branch] を削除

次のステップ:
- 次の作業を始める → /git-branch
```
