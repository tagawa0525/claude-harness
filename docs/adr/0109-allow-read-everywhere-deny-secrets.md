---
type: ADR
status: accepted
date: 2026-11-28
requires: []
supersedes: []
superseded-by:
issues: ["#428"]
description: 静的な設定（claude-settings.json）の permissions.allow に範囲を付けない Read を置き、Read、Grep、Glob をどのパスでも確認なしにする。分かっている秘密の置き場（SSH、AWS、gh、GnuPG、sops の age 鍵と復号した秘密、git と netrc の資格情報、Claude の資格情報、.env）の Read は deny で拒む
---

# ADR-0109: 読み取りはどのパスでも確認なしにし、秘密の置き場だけ deny で拒む

## 背景

ユーザーが「workfolder 外でも読み取りは基本許可します」と決め、全プロジェクトに効かせるよう指示した。作業フォルダの外を
Read、Grep、Glob で読むたびに確認が出て、作業が止まっていた。

配る設定は、`Bash(curl:*)`、`Bash(gh api:*)`、`WebFetch(domain:github.com)` などの外へ送るツールを確認なしにしている。
Read をどこでも確認なしにすると、プロンプトインジェクションで秘密のファイルを Read させられたとき、中身が人の確認を通らずに
外へ出る経路が増える（ローカルの `/code-review` の指摘）。Bash の `cat`、`grep`、`find` は前からパスを絞らずに許可していて、
同じ経路がすでにある。

## 検討した案

- A. 範囲を付けない `"Read"` だけを allow に置く。確認は出なくなる。秘密の置き場も Read で確認なしに読める
- B. 範囲を絞った allow（`Read(//c/Users/tagaw/**)` など）。ホームのパスが機械ごとに違い（Windows と Linux）、配る設定 1 つで
  書き分けられない。ホームの外（`/nix/store`、`/etc`）を読むたびに確認がまた出る
- C. `"Read"` を allow に置き、秘密の置き場の Read を deny に置く。`~/` と `//**/` で書けるので、機械ごとのホームの違いが出ない。
  Windows だけの置き場（`~/AppData/Roaming/` の下）は別の行に書く
- D. C の deny を ask にする。秘密を読む場面が無いのに確認が出るたびに許せば、確認が形だけになる
- E. claude-hooks の PreToolUse で、秘密の置き場の一覧を 1 か所に持ち、Read、Grep、Glob と Bash の引数を照らして拒む。Bash の
  経路も塞げる。hook の実装とテストが要り、ユーザーの指示（Read の確認をなくす）の範囲を超える

## 決定と理由

C を採る。ユーザーの指示（どこでも読める）を満たしたまま、Read で増える経路のうち、外へ出ると取り返せない秘密の置き場で、
分かっているものを塞ぐ。deny にする置き場は `claude-settings.json` の `permissions.deny` の `Read(...)` の行。一覧は
すべての置き場を網羅しない。E は Bash の経路も塞ぐが、前からある経路の
話で、この決定と別に #428 で扱う。

### 帰結

- 秘密の置き場を読ませたいときは、ユーザーが中身を手で渡す
- Claude Code の権限判定は、Read の deny を Bash の引数のパスにも、変数と cd の後の相対のパスまで解いて当てるが、
  `$(...)` の置換、xargs が標準入力から足すパス、`bash -c` の中には当てない（#428 のコメント）。その形は、hook の
  block-secret-read がこの deny の一覧を読んで拒む（ADR-0110）
- deny の一覧に無い資格情報の置き場（`~/.docker/config.json`、`~/.kube/config` など）は、Read で確認なしに読める。使い始めたら
  一覧に足す
- deny はパスの文字列で照らすので、symlinkや大文字と小文字を変えた書き方（Windows）で同じファイルを指すと、
  すり抜けうる
- `.env` と `.env.*` を拒む。`.env.example` のような秘密を持たないテンプレートも拒まれ、読ませるには手で渡す。`.envrc`（direnv）は
  秘密を置かない慣例なので拒まない

### 確認

配備の後、Linux と Windows のそれぞれで、次を見る。

- 作業フォルダの外のファイルを Read して確認が出ない
- `~/.ssh` の下と、作業フォルダの外の `.env` を Read して拒まれる
- `~` を起点にした Grep と Glob が、`~/.ssh` の下の中身を返さない

## 補足

- 2026-11-29: 案 E のうち Bash の経路は、ADR-0110 で hook（block-secret-read）が扱う。一覧はこの ADR の deny の行だけに
  持ち、hook が実行時に読む。Read、Grep、Glob は今までどおりこの deny が拒む
