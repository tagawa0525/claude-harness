---
status: accepted
date: 2026-10-24
requires: [ADR-0014, ADR-0050]
supersedes: []
superseded-by:
issues: []
---

# ADR-0058: コミットと merge の本文の語の検査は Markdown として読む

## 背景

コミットと merge の本文の語の検査（ja-terms-message）は、本文を `COMMIT_EDITMSG.txt` として textlint に渡し、
テキストとして読んでいた。textlint は `--stdin-filename` の拡張子でパーサーを選ぶので、バッククォートで囲んだ
識別子やファイル名（`壁時計` など）の中の語も崩れた語として拒んだ。文書（.md）の検査は Markdown として読み、
コードスパンを対象外にする。2026-10-23〜24 の xlc のマージで、本文のコードスパンが拒まれて書き直した。

## 検討した案

- 案 A: ファイル名を `COMMIT_EDITMSG.md` にして Markdown として読む。変更は 1 語で、文書と同じ規則になる
- 案 B: .txt のままコードスパンを除いてから渡す。textlint の設定（`@textlint/text`）にコードスパンを除く
  項目は無く、除くには自前でバッククォートを解釈する前処理が要る
- 案 C: 案 A のまま、prh の `checkLink` / `checkBlockQuote` / `checkEmphasis` を true にして、引用、リンクの
  文字列、強調を検査に戻す（textlint.nix の `rules.prh`）。文書の検査も同じ設定で動くので、xlc の docs などに
  新しく引っかかる箇所が出る。この ADR では入れない

## 決定と理由

案 A。ユーザーが案 A か案 B かを選ぶよう示し（2026-10-24）、変更が小さく文書の検査と同じ規則になる案 A を選んだ。

### 帰結

Markdown の規則が本文にも及ぶ。textlint 15.7.1 で確かめた、検査しない範囲（文書と同じ）は、インデントした
コードブロック（空行の後の 4 つ以上の空白の字下げ）、フェンス、ブロックの HTML、引用（`>`）、リンクの文字列、強調（`*語*`。
`_語_` は前後が日本語のとき強調と読まれず検査する）。閉じていないフェンスや HTML の行があると、以降の本文を検査しない。リストと表、段落は検査する。
文書の検査も同じ範囲を見落とす。引用、リンクの文字列、強調の見落としが問題になったら、前処理（案 B）
でなく案 C の設定を使う（prh 6.0.0 の既定は、Emphasis など 3 種の中を検査しない。textlint-rule-prh の
lib/textlint-rule-prh.js の createIgnoreNodeTypes）。

### 確認

modules/home/parts/tests/ja-terms.sh の 6 節が、コードスパンの中は通し、外は止めることと、引用、リンクの
文字列、強調、フェンスは通し、見出しとリストは止めることを 1 件ずつ確かめる（prh の既定が変わったときに気づく）。
