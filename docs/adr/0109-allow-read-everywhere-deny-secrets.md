---
type: ADR
status: accepted
date: 2026-11-28
requires: []
supersedes: []
superseded-by:
issues: []
description: 静的な設定（claude-settings.json）の permissions.allow に範囲を付けない Read を置き、Read、Grep、Glob をどのパスでも確認なしにする。秘密の置き場（~/.ssh、~/.aws、~/.config/gh、~/.gnupg、~/.claude/.credentials.json、.env*）の Read は deny で拒む
---

# ADR-0109: 読み取りはどのパスでも確認なしにし、秘密の置き場だけ deny で拒む

## 背景

ユーザーが「workfolder 外でも読み取りは基本許可します」と決め、全プロジェクトに効かせるよう指示した。作業フォルダの外を
Read、Grep、Glob で読むたびに確認が出て、作業が止まっていた。Bash の `cat`、`grep`、`ls`、`find` は、すでにパスを絞らずに
許可している。

配る設定は、`Bash(curl:*)`、`Bash(gh api:*)`、`WebFetch(domain:github.com)` などの外へ送るツールも確認なしにしている。読み取りを
すべて確認なしにすると、プロンプトインジェクションで秘密のファイルを読まされたとき、中身が人の確認を通らずに外へ出る経路ができる
（ローカルの `/code-review` の指摘）。

## 検討した案

- A. 範囲を付けない `"Read"` だけを allow に置く。確認は出なくなる。秘密の置き場も確認なしで読め、上の経路が残る
- B. 範囲を絞った allow（`Read(//c/Users/tagaw/**)` など）。ホームのパスが機械ごとに違い（Windows と Linux）、配る設定 1 つで
  書き分けられない。ホームの外（`/nix/store`、`/etc`）を読むたびに確認がまた出る
- C. `"Read"` を allow に置き、秘密の置き場の Read を deny に置く。deny は `~/` で書けるので機械ごとの違いが出ない
- D. C の deny を ask にする。秘密を読む場面が無いのに確認が出るたびに許せば、確認が形だけになる

## 決定と理由

C を採る。ユーザーの指示（どこでも読める）を満たしたまま、外へ出ると取り返せない秘密の置き場だけを塞ぐ。deny にする置き場は
`~/.ssh`、`~/.aws`、`~/.config/gh`、`~/.gnupg`、`~/.claude/.credentials.json` と、作業フォルダの下の `.env*`。

### 帰結

- 秘密の置き場を読ませたいときは、ユーザーが中身を手で渡す
- deny は Read の規則なので、Bash の `cat ~/.ssh/id_ed25519` は塞がない。Bash の読み取りの許可はこの決定の範囲の外で、変えていない
- `.env*` は作業フォルダから見た相対の規則で、作業フォルダの外の `.env` は塞がない

### 確認

配備の後、作業フォルダの外のファイルを Read して確認が出ないこと、`~/.ssh` の下を Read して拒まれることを見る。
