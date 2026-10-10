---
name: git-merge
description: featureブランチをローカルでmainにマージ。GitHubを経由しないリポジトリ用。チェック・レビュー・rebase・マージコミット・後片付けまで行う。
# model 未指定: レビュー指摘への対応、コンフリクト解決、マージコミットメッセージの判断が必要なためセッションモデルを継承する
argument-hint: [branch]
allowed-tools:
  - Bash(~/.claude/scripts/git-merge-state.sh*)
  - Bash(~/.claude/scripts/post-merge-cleanup.sh*)
  - Bash(~/.claude/skills/language-checks/scripts/run-checks.sh*)
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git show*)
  - Bash(git switch*)
  - Bash(git rebase*)
  - Bash(git merge*)
  - Bash(git rev-list*)
  - Bash(git worktree*)
---

# Git Merge Command

GitHub リモートを持たないリポジトリで、feature ブランチをローカルの main にマージする。
GitHub での PR の流れ（CI → 自動レビュー → `gh pr merge --merge`）を、ローカルの手順に置き換えたもの。

## 現在の状態

!`~/.claude/scripts/git-merge-state.sh $ARGUMENTS`
!`git log --oneline --graph --all -20`

$ARGUMENTS のブランチ（省略時は現在のブランチ）が対象。上が `ERROR:` で終わっていれば、その指示に従う
（main 自身、GitHub リモートあり、未コミットの変更は、ここで止まる）。

## 守られるゲート

次は hook が強制する。通らないときは理由に従って直す（文言や書き方を変えて迂回しない）:

- main への `git merge` は `--no-ff` で、main の上に rebase 済みのブランチだけ（guard-git-merge）
- 新しい ADR を 2 件以上加えるブランチはマージしない。決定ごとにブランチを分ける（pre-git-merge-check、/topic-triage）
- マージ結果が `run-checks.sh --merge` を通らなければマージコミットを作らない（git の pre-merge-commit）

## 判断が要る手順

### main がまだない（`DEFAULT: none`）

`ROOT_COMMITS` が 1 つで、初期化だけの内容であることを `git show` で確認してから、ルートコミットを指す main を作る:

```bash
git branch main [root-commit]
```

ルートコミットに初期化以外の変更が含まれていれば、作らずにユーザーに確認する。

### 積み重なり（`STACKED_ON` が none でない）

対象は、まだマージしていない別のブランチの上に積まれている。土台のブランチから順にこの手順でマージし、
対象は後で rebase する。

### rebase（`BEHIND` が 0 でない）

```bash
git switch [branch]
git rebase main
```

コンフリクトしたら内容と解決案を示し、解決して `git rebase --continue` するか
`git rebase --abort` するかを決める。

### チェック（CI の代わり）

対象ブランチで品質チェックを実行する。マージ時にも hook が走るが、ここで先に直しておく:

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh
```

失敗したら原因を直してコミットし、もう一度チェックする。通らないままマージしない。

### レビュー（自動レビューの代わり）

`/code-review` で main との差分をレビューし、指摘に対応してからマージする:

- 指摘を修正したら、コミットして rebase の要否とチェックからやり直す
- 対応しない指摘は、その理由をユーザーに伝える

### マージコミットメッセージ

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

### マージ

main へ移り、必ずマージコミットを作る（GitHub での `--merge` と同じ履歴になる）:

```bash
git switch main
git merge --no-ff [branch] -m "$(cat <<'EOF'
[マージコミットメッセージ]
EOF
)"
```

`git merge` は `-F -`（標準入力からの読み込み）に対応していないので、`-m` に渡す。

## 後片付け

対象の worktree とブランチを削除する（変更が残っていたり、未マージだったりすれば失敗する。確認して対処する）:

```bash
~/.claude/scripts/post-merge-cleanup.sh [branch]
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
