---
status: proposed
date: 2026-10-22
requires: [ADR-0009]
supersedes: []
superseded-by:
issues: []
---

# ADR-0051: 一部を置き換えた ADR と置き換えられた ADR を、フロントマターの amends と amended-by で両方向に結ぶ

## 背景

ADR の全体を置き換えるときは、`supersedes` と `superseded-by` で両方向に結び、`design-records check` が一致を確かめる
（ADR-0009）。決定の一部だけを置き換えるときは、古い ADR の status を変えず、新しい ADR が本文に「これは ADR-NNNN の
「…」の部分を置き換える」と書き、古い ADR を `requires` に入れる（xlc の docs/adr/README.md「運用」）。nixfiles では、
古い ADR の補足に日付つきの行を足して新しい ADR を指すこともある（ADR-0042 と ADR-0020、ADR-0025、ADR-0041）。

古い ADR から新しい ADR への参照は、どちらの形でも機械では確かめていない。xlc では、一部を置き換えられた ADR-0013、
ADR-0095、ADR-0156、ADR-0164、ADR-0173 のどれにも、置き換えた ADR-0129、ADR-0132、ADR-0182、ADR-0183 への参照が無い
（2026-10-22、682dead38）。古い ADR を読んだ人は、決定の一部がもう効いていないことに気づけない。

- 採用中の ADR を読めば、今の決定が分かる（置き換えられた部分があれば、そこから新しい ADR に辿れる）
- 判定は本文の語の解析に頼らず、フロントマターで閉じる
- 確定した ADR の本文は変えない（ADR-0009）

## 検討した案

- 案 A: 新しい ADR に `amends: [ADR-NNNN]`、古い ADR に `amended-by: [ADR-NNNN]` を書き、`check` が両方向の一致を確かめる
- 案 B: 古い ADR にだけ `amended-by` を書き、新しい ADR の本文の「ADR-NNNN の「…」の部分を置き換える」の文と照らす
- 案 C: 古い ADR の補足に日付つきの行を足す（nixfiles の今の形）
- 案 D: 古い ADR を `requires` に挙げた ADR を、逆向きに引く

## 決定と理由

採用: 「案 A」。

- `amends` は、この ADR が一部を置き換えた ADR の一覧。`amended-by` は、この ADR の一部を置き換えた ADR の一覧。どちらも
  `[ADR-NNNN, …]` の形で、挙げた ADR は存在しなければならない
- 両方向の一致は、`supersedes` と `superseded-by` と同じく、置き換える側が効力を持ってから（accepted、superseded、
  deprecated）求める。効力を持った ADR の `amends` に挙げた ADR は、`amended-by` にその ADR を挙げる。`amended-by` に
  挙げた ADR は、`amends` にこの ADR を挙げ、効力を持っている。proposed の ADR が `amends` に挙げた ADR の決定は、
  まだ全体が効いているので、`amended-by` を書かない
- 古い ADR の status は変えない（決定の残りは効いている）。置き換えた部分は、新しい ADR が本文に古い ADR の語で書く
  （人が書く。検査はしない）
- 2 つのキーは、すべての ADR に求めず、書いてもよいキーにする。一部の置き換えはまれで（xlc で 5 件、2026-10-22）、
  空のキーを求めると、4 つのプロジェクトのすべての ADR に 2 行を足すことになる。`design-records new` のテンプレートにも
  書かない
- 確定した後も、`amended-by` と `amends` を書き足してよい。`amended-by` は、`superseded-by` と同じく、新しい ADR が決まった
  ときに古い ADR に書く。`amends` は新しい ADR を書くときに決まるが、この決定の前に確定した ADR（xlc の ADR-0129 など）は、
  本文に書いた置き換えをフロントマターにも書くために足す。どちらも相手の ADR との一致を検査するので、片方だけの書き足しは
  error になる
- `requires` は変えない。新しい ADR は、置き換えなかった部分を前提にするので、古い ADR を `requires` にも挙げてよい（検査は
  求めない）

採らなかった案:

- 案 B: 本文の文の形（「ADR-NNNN の「…」の部分を置き換える」）を検査が読むことになる。書き方の揺れ（xlc の ADR-0182 は
  「ADR-0156 の帰結の「…」を置き換える」、ADR-0183 は「ADR-0173 の「…」の段落と、…の行を置き換える」、2026-10-22）
  で読み落とす
- 案 C: 書き忘れても検査で気づけない。補足は日付つきの追記で、どの ADR が一部を置き換えたかを grep で一覧にしにくい
- 案 D: `requires` は前提を挙げるキーで、置き換えずに前提にしただけの ADR と区別できない

これは ADR-0009 の「フロントマター」の、キーを `requires` / `supersedes` / `superseded-by` / `issues` とする部分と、
「運用」の「確定した後に直してよいフロントマターは、`status` に加えて `superseded-by` と `date`」の部分を置き換える。
`amends` と `amended-by` を書いてもよいキーとして足し、確定した後にも書き足してよい。ほかの部分は変わらない。

### 帰結

- 良い点: 一部を置き換えられた ADR から、置き換えた ADR に辿れる。書き忘れは、置き換える側が効力を持った時点で
  `check` が error にする
- 悪い点: 置き換えた部分の範囲（古い ADR のどの段落か）は、新しい ADR の本文にしか無く、検査しない
- 実装が決定に追いついていない点: xlc の 5 件の古い ADR と、ADR-0129、ADR-0132、ADR-0182、ADR-0183 のフロントマターは、
  この変更の後に xlc で書き足す。nixfiles の既存の一部の置き換え（ADR-0042 など、補足から指しているもの）は、まだ
  フロントマターで結んでいない

### 確認
