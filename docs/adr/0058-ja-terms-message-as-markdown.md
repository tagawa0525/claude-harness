---
status: proposed
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

## 決定と理由

案 A。ユーザーが案 A か案 B かを選ぶよう示し（2026-10-24）、変更が小さく文書の検査と同じ規則になる案 A を選んだ。

### 帰結

Markdown の規則が本文にも及ぶ。インデントしたコードブロック、フェンス、HTML の中は検査しない（文書と同じ）。
本文の字下げした行（4 つ以上の空白の字下げ）に崩れた語を書いても通る。

### 確認

modules/home/parts/tests/ja-terms.sh の 6 節が、コードスパンの中は通し、外は止めることを確かめる。
