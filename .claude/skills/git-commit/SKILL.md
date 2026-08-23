---
name: git-commit
description: ステージされた変更をConventional Commits形式でコミット。メッセージ省略時は自動生成。
# 差分分析とコミットメッセージ生成の品質を優先
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
  - Bash(git stash*)
  - Bash(git switch*)
  - Bash(git worktree*)
  - Bash(python3*)
  - AskUserQuestion
---

# Git Commit Command

ステージされた変更に対してConventional Commits形式のコミットメッセージを作成しコミットを実行。

## 現在の状態

!`git branch --show-current`
!`git status --short`
!`git diff --cached --stat`

## mainブランチからの自動退避

現在のブランチが `main` または `master` の場合、コミットはhookで禁止されている。
**ユーザーには質問せず**、以下の手順で自動的に退避してからコミットする。
ブランチはマージ後すぐ削除される使い捨てであり、名前の確認も不要。

### Step 1: ブランチ名の自動生成

変更内容（変更ファイルと差分）から意図を読み取り、git-branch スキルと同じ
命名規則（`feat/` `fix/` `refactor/` `docs/` `chore/` + kebab-case）で
ブランチ名を自動決定する。候補の提示はしない。

### Step 2: リモート有無で退避先を分岐

判定は gh-wait-review.sh の exit 2 と同一基準:

```bash
git remote -v | grep -q 'github\.com'
```

**GitHubリモートあり（PRフロー適用）→ worktreeを自動作成:**

レビュー待ちの間ブランチが生き続けるため、worktreeに隔離して
元ディレクトリのmainを空ける。未コミット変更はstashで移送する
（stashはworktree間で共有されるため機械的に移せる）:

```bash
git stash push -u -m "wip: move to worktree"
git worktree add ../[repo-name]-[branch-dirname] -b [branch-name]
git -C ../[repo-name]-[branch-dirname] stash pop
```

- `[branch-dirname]` はブランチ名の `/` を `-` に変換したもの
- 未コミット変更がない場合（stash push が "No local changes" を返す場合）は
  stash pop をスキップする
- stash pop はステージ状態を復元しないが、この後コミット単位で
  再ステージするため問題ない
- 以降のステージング・コミット・プッシュはすべてworktree側で行う
- fork運用リポジトリではworktree作成後に `gh repo set-default` を確認する
  （git-worktree スキル参照）

**GitHubリモートなし（PRフロー適用外）→ その場でfeatureブランチ:**

コミット→ローカルマージ→ブランチ削除が即時に完結し、worktree隔離の
利点がないため、カレントディレクトリで切り替える（未コミット変更は
そのまま引き継がれる）:

```bash
git switch -c [branch-name]
```

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

## Markdown自動修正（コミット前）

ステージされたファイルに `.md` ファイルが含まれる場合、コミット前に自動修正を実行:

```bash
git diff --cached --name-only --diff-filter=ACM | grep '\.md$' | \
  xargs -r python3 ~/.claude/skills/git-commit/scripts/fix-markdown-lint.py
```

修正されたファイルを再ステージ:

```bash
git diff --cached --name-only --diff-filter=ACM | grep '\.md$' | xargs -r git add
```

このスクリプトは `markdownlint --fix` では対応できない以下を修正:

- **MD040**: コードブロックの言語をヒューリスティックで推測・付与
- **MD060**: CJK全角文字を考慮したテーブル列幅の整列

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
