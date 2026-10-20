---
status: proposed
date: 2026-10-20
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0038: worktree-add は base を決める前に origin を fetch し、失敗したら worktree を作らない

## 背景

2026-10-20、古い `origin/main` から作ったブランチで、別のセッションが既に main に入れていた変更（コミット 11ea045d）と
同じ内容を進め、PR #376 を出して閉じた。`worktree-add.sh` は fetch をせず、新しいブランチの起点（`BASE`）に
手元の追跡ブランチをそのまま使っていた。気付いたのはマージ直前の rebase だった。

worktree を作る時点では、`origin/<既定ブランチ>` が最新かどうかが起点の入力になる。PR を出す前の確認
（`base-moved.sh`）は、その後に main が進んだことを知らせる補助である。

## 決定の要因

- 古い起点で始めると、重複した作業が丸ごと無駄になる
- オフラインや認証切れで、既存のブランチをチェックアウトするだけの作業まで止めたくない
- 引数なしの `git fetch` は今のブランチの上流のリモートへ行く。起点は `origin/*` なので、上流が別のリモートだと更新されない

## 検討した案

- (a) fetch に失敗したら警告して、古い起点で進む（`base-moved.sh` の方式）。起点が古いまま進むので、事故の再発を
  許す。警告は流れて読まれない
- (b) fetch に失敗したら worktree を作らずに止める（採用）
- (c) fetch しない（従来）。事故の原因そのもの

## 決定と理由

(b) を採る。GitHub リモートがあり、ローカルにそのブランチが無く `--from` も無いとき（起点を使うか、リモートの
ブランチを探すとき）だけ、ブランチの状態を調べる前に `git fetch -q origin` を実行する。origin を明示するのは、
上流が別のリモートのブランチで作業しているときも `origin/*` を更新するため。失敗したら `ERROR:` で exit 1 にして
worktree を作らない。`post-merge-cleanup.sh` が fetch の失敗で止まるのと同じ扱いである。

`base-moved.sh` が WARN で続けるのに `worktree-add.sh` が失敗させる非対称は、入力か補助かの違いによる。
worktree の起点は、古いと重複作業になる入力なので、確かめられないまま進めない。PR 前の通知は、既に済んだ作業の
補助で、失敗させると PR を出せなくなるだけで得るものが無い。ただし WARN のときは比較が済んでいないので、
`/gh-pr-create` は「出力が空」を「進みなし」と読まず、未確認として扱う。

## 帰結

- 新しいブランチの worktree は、オフラインでは作れない。作れるのは、ローカルにあるブランチと `--from` 指定だけである
  （リモートにだけあるブランチも、探すために fetch するので作れない）
- 比べる先は origin の既定ブランチに限る。GitHub リモートが複数ある fork 運用でも、upstream の既定ブランチとは比べない
- fetch のタイムアウトと認証プロンプトでの待ちは、今の要件の外として扱い、何も対処しない（git の既定に従う）

## 確認

`.claude/tests/scripts/git-workflow.sh` の worktree-add の節（進んだ origin/main から作る、fetch の失敗で作らない、
上流が別のリモートでも origin を fetch する）と、`.claude/tests/scripts/base-moved.sh`。
