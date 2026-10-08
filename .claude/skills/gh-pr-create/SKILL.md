---
name: gh-pr-create
description: GitHub PRを作成。変更内容からPRタイトルと本文を自動生成。
# 差分の要約とPR本文生成の品質を優先
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
---

# GitHub PR Create Command

GitHub Pull Requestを作成する（gh CLI使用）。このスキルの本体はタイトルと本文を書くこと。

## 事前確認

!`gh auth status 2>&1 | head -3`
!`git status --short`
!`git branch -vv`
!`git log '@{upstream}..HEAD' --oneline 2>/dev/null || echo "(上流ブランチ未設定)"`

## トピックの確認

!`~/.claude/scripts/branch-topics.sh`

PR の Why を1文で書く。2文要る（無関係な決定や話題が混ざっている）なら PR を作らず /topic-triage で分ける。

## 未プッシュコミットの確認

未プッシュコミットがある（または上流が未設定）なら先にプッシュする（上流は自動設定される）:

```bash
git push
```

`pre-pr-create-check` hook は未プッシュのまま `gh pr create` すると deny する
（gh が対話的に push 先を尋ねて固まるのを防ぐ）。hook はコマンド実行**前**の状態を見るので、
`git push && gh pr create …` のように 1 コマンドに繋ぐと push 前の状態で deny される。
push と `gh pr create` は必ず別々の Bash 呼び出しで実行する。

## PR内容の生成

ベースブランチとの差分を分析:

```bash
git log main..HEAD --oneline
git diff main..HEAD --stat
```

### PRタイトル

- 最初のコミットメッセージまたは変更の要約から生成
- 70文字以内

### PR本文テンプレート

見出し `## Summary` / `## Changes` / `## Tests` は必須。本文は、同じリポジトリへの PR なら日本語、
fork から上流へ出す PR（`-R owner/repo --head my-account:branch`）なら相手のリポジトリの言語
（ふつう英語）で書く:

```markdown
## Summary
[変更内容の要約]

## Changes
- [主要な変更点をリスト]

## Tests
- [ ] 動作確認済み
- [ ] テスト追加/更新済み
```

## PR作成

`--title` と `--body` は必ず指定する（`--fill` や本文省略はエディタが開いて止まる）。
`--web` は使わない。

### 通常のPR

```bash
gh pr create --title "[タイトル]" --body "$(cat <<'EOF'
## Summary
[要約]

## Changes
- [変更点]

## Tests
- [ ] 動作確認済み
EOF
)"
```

`--draft` / `--reviewer` が指定されていればそのまま付ける。

## 完了

`gh pr create` が出力したURLを報告する（ブラウザは開かない）。
URLを取り直す場合:

```bash
gh pr view --json url -q .url
```

## 次のステップ

```text
✅ PRを作成しました: [URL]

次のステップ:
- Copilotのレビューを待つ場合 → ~/.claude/scripts/gh-wait-review.sh（約 10 分。`run_in_background=true` で実行）
- Copilotレビュー/CIの状況を確認する場合 → /gh-actions-check
- レビューコメントに対応する場合 → /gh-pr-review
- レビュー後にマージする場合 → /gh-pr-merge
- 状態を確認する場合 → /git-info
```
