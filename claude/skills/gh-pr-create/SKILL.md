---
name: gh-pr-create
description: GitHub PRを作成。変更内容からPRタイトルと本文を自動生成。
model: sonnet
argument-hint: [--draft] [--reviewer REVIEWER]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(git diff*)
  - Bash(git push*)
  - Bash(gh pr*)
  - Bash(gh auth*)
  - Bash(~/.claude/scripts/branch-topics.sh*)
  - Bash(~/.claude/scripts/review-level.sh*)
  - Bash(~/.claude/scripts/base-moved.sh*)
---

# GitHub PR Create Command

GitHub Pull Requestを作成する（gh CLI使用）。本体はタイトルと本文を書くこと。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv`
!`git log '@{upstream}..HEAD' --oneline 2>/dev/null || echo "(上流ブランチ未設定)"`

## トピックの確認

!`~/.claude/scripts/branch-topics.sh`

PR の Why を1文で書く。2文要る（無関係な決定や話題が混ざっている）なら PR を作らず /topic-triage で分ける。

## 比較元の進みの確認

ブランチ名を渡して実行する（引数なしは cwd のブランチを見る）。積み重なったブランチは
`--base origin/<土台のブランチ>` を付ける:

```bash
~/.claude/scripts/base-moved.sh <ブランチ名>
```

出力の `NEXT:` と `UNVERIFIED:` の行に従う。どちらも無ければ進める。

## ローカルレビュー（PR を出す前）

PR を出す前にローカルの `/code-review` を回し、直す指摘が無くなってから出す。

```bash
~/.claude/scripts/review-level.sh
```

- 出力の `NEXT:` の `/code-review` を回す。別のブランチの上に積んでいるときは `review-level.sh` に `--base <土台のブランチ>` を
  付ける
- レビューが対象を見つけられなかった・実行できなかった返答は、指摘ゼロでなく未実施として扱う
  （`[]` は、返答が差分を読んだと示しているときだけ指摘ゼロ）
- 直すのは /git-merge の「レビュー」の節の「直す指摘の基準」の blocking だけ。直したらコミットし、もう一度回すかを
  同じ節の「終わり」の項で決める。その周に blocking が無ければ終わり。直さない指摘は「直す指摘の基準」のとおり扱い、
  その指摘の行（`再現なし: <指摘の要旨>` など）を PR 本文の Tests に書く
- claude-hooks の抜け道の指摘は、/git-merge の「レビュー」の節の同じ項に従う
- 文書にはコードの挙動を重複して書かず、コードへの参照（ファイルと関数名）を書く
- 別の PR を先にマージしないと成り立たないときは、PR 本文の Summary に先行 PR を書く（他リポジトリは `owner/repo#N`）

## 未プッシュコミットの確認

未プッシュコミットがある（または上流が未設定）なら、`gh pr create` の前にプッシュする:

```bash
git push
```

## PR内容の生成

```bash
git log main..HEAD --oneline
git diff main..HEAD --stat
```

- タイトル: 最初のコミットメッセージまたは変更の要約から。70文字以内
- 本文: 見出し `## Summary` / `## Changes` / `## Tests` は必須。同じリポジトリへの PR なら日本語、
  fork から上流へ出す PR（`-R owner/repo --head my-account:branch`）なら相手のリポジトリの言語（ふつう英語）で書く

## PR作成

`--draft` / `--reviewer` が指定されていればそのまま付ける。

```bash
gh pr create --title "[タイトル]" --body "$(cat <<'EOF'
## Summary
[変更内容の要約]

## Changes
- [主要な変更点をリスト]

## Tests
- [ ] 動作確認済み
- [ ] テスト追加/更新済み
EOF
)"
```

## 完了

URL の報告と次のステップは、`gh pr create` に hook が添える案内に従う。
