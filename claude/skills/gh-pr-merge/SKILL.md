---
name: gh-pr-merge
description: GitHub PRをマージ。
# マージコミットメッセージ（Why/What/Impact）生成の品質を優先
model: sonnet
argument-hint: [PR番号]
allowed-tools:
  - Bash(git status*)
  - Bash(git branch*)
  - Bash(git log*)
  - Bash(gh pr*)
  - Bash(gh auth*)
  - Bash(~/.claude/scripts/gh-wait-review.sh*)
  - Bash(~/.claude/scripts/gh-wait-checks.sh*)
  - Bash(~/.claude/scripts/post-merge-cleanup.sh*)
  - Bash(~/.claude/scripts/branch-topics.sh*)
---

# GitHub PR Merge Command

GitHub の PR をマージする。機械的に判定できる条件（マージ方式、本文の見出し、CI、未解決スレッド、head の遅れ）は
`pre-merge-check` hook が deny して修正方法を示すので、ここでは判断が要るものだけを扱う。/delegate の担当の PR は、
親の許可の後に担当がマージする（/delegate の受ける側の手順 10）。

## 事前確認

下の `!` 行は cwd のリポジトリを見る。$ARGUMENTS の PR が別のリポジトリのものなら、その出力は読み飛ばし、状態は
`gh pr view <PR の URL>` で見る。後の `gh pr view`、`gh pr merge` にも、番号でなく PR の URL を渡す（番号は cwd の
リポジトリで解かれる）。

!`gh auth status 2>&1 | head -3`
!`git branch -vv`

## マージ前の判断

!`~/.claude/scripts/branch-topics.sh`

$ARGUMENTS の PR（省略時は現在のブランチの PR）について:

- 上の出力から、マージのメッセージの Why を 1 文で書けるか確かめる。2 文要るならマージせず /topic-triage で分ける。
  PR の head が今のブランチでなければ、`git fetch` のあと `~/.claude/scripts/branch-topics.sh origin/<base> origin/<head>` で見る
- head にローカルレビューの記録（`record-local-review.sh`、High なし）があれば、Copilot のレビューを待たずにマージできる。
  記録が無く自動レビューも未着なら `~/.claude/scripts/gh-wait-review.sh <PR の URL>` で待つ（約 10 分。URL は PR の
  リポジトリの worktree で `cd <worktree> && gh pr view [PR番号] --json url -q .url`）
- CI が実行中か、push の直後で check が未登録なら `~/.claude/scripts/gh-wait-checks.sh <PR の URL> <push したコミットの SHA>` で待つ
- 待ちの出力の次の一手（`次の一手:`、`NEXT:`）に従う
- 指摘への対応が残っていれば /gh-pr-review に戻る
- 対応しなかった指摘（マージの本文の `再現なし:` の行）が会話に無ければ、ローカルレビューの記録のコメントの「指摘:」から
  拾う（`gh pr view <PR の URL> --comments`）
- head がレビューの後に変わっていても hook は止めず、変わったファイルをマージの後に知らせる。もう 1 周回すかは
  マージの前に /git-merge の「直す指摘の基準」の「終わり」の項で決め、回さないなら理由をマージの本文の Impact に書く

## マージコミットメッセージ

PR の内容を把握してから書く:

```bash
gh pr view [PR番号または URL] --json title,body,commits,files,headRefName
```

```text
Merge: [PRタイトルを簡潔に要約]

## Why
[PRの目的・背景を1-2文]

## What
- [主要な変更点を3-5項目]

## Impact
[影響を与えるモジュール/機能]

PR: #[番号]
```

## マージ実行

```bash
gh pr merge [PR番号または URL] --merge --delete-branch \
  --subject "Merge: [subject]" \
  --body "[body]"
```

## クリーンアップ

```bash
~/.claude/scripts/post-merge-cleanup.sh [headRefName]
```

worktree の削除、main の最新化、ローカルとリモートのブランチの削除を行う（残したときは `REMOTE_BRANCH: kept` に理由が出る）。
PR のリポジトリと cwd が違えば、`cd <PR のリポジトリ> && ~/.claude/scripts/post-merge-cleanup.sh [headRefName]` で呼ぶ。
ERROR で止まったら、出力の原因を直してやり直す。未コミットの変更や履歴の分岐で失敗したとき（git のメッセージだけで、
ERROR の行は出ない）も、原因を確認して対処する。

出力の `NOTE:` の行（rebuild か claude-sync が要る）は、完了メッセージでそのままユーザーに伝える。マージ済みの PR を
痕跡にした `NOTE:` の行は、その URL がマージした PR かを確かめ、違えば PR のリポジトリでやり直す。

## 完了メッセージ

```text
✅ PRをマージしました。

クリーンアップ完了:
- worktree [path] を削除（該当する場合）
- ブランチ [branch] を削除（リモートを残した場合はその理由）
- [post-merge-cleanup の NOTE: の行]（出た場合）
```
