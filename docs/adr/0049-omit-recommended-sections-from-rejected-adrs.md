---
status: proposed
date: 2026-10-22
requires: [ADR-0009]
supersedes: []
superseded-by:
issues: []
---

# ADR-0049: 採らなかった ADR には推奨の節を求めない

## 背景

`design-records check` は、すべての ADR に推奨の節（帰結、確認）を求め、無ければ warning を出す（ADR-0009）。xlc の
ADR-0091 と ADR-0092 は、移行の基準により独立した ADR にしなかった決定の番号を再利用しないために、`status: rejected` の
ファイルとして残したものである。確定した後は本文を変えられないので、この 2 件は毎回「帰結」と「確認」の warning を出し続け、
今のコミットの warning が埋もれる。

- 採らなかった ADR には、確かめる実装も、決定が起こす帰結も無い
- 確定後の ADR の本文は変えられない（ADR-0009）ので、warning を消すために書き足せない
- 新しい rejected の ADR に対する規則は弱めない

## 検討した案

- rejected の ADR には、帰結と確認のどちらも求めない
- rejected の ADR には、確認だけを求めない。帰結は求める
- 古い ADR に節を足すことを、確定後にも許す
- 古い ADR の warning を、ADR ごとの除外の設定で消す

## 決定と理由

採用: 「rejected の ADR には、帰結と確認のどちらも求めない」。

- 求めない推奨の節は、帰結と確認の 2 つとも。必須の節（背景、検討した案、決定と理由）は rejected でも求め、無ければ
  error にする。採らない理由は、必須の「決定と理由」に書く。新しい rejected の ADR は、`design-records new` のテンプレートで
  作るので、帰結と確認の見出しも持つ。見出しがあれば検査は何も言わず、あっても構わない
- 他の status（withdrawn を含む）の規則は変えない。withdrawn は取り下げた ADR で、本文に経緯が要る
- 「確認」は、決定が成り立つことを示すものなので、採らなかった決定には対象が無い。「帰結」も、採った決定の影響を書く節である

採らなかった案:

- 帰結だけを求める: xlc の ADR-0091 と ADR-0092 は帰結の節を持たず、理由を「決定と理由」に書いている。確定後は直せないので、
  warning が残り、目標を満たさない
- 確定後の追記を許す: 確定した ADR は変えられないという規則（ADR-0009）の例外が増える。warning を消すためだけに、
  本文に中身のない節を足すことになる
- ADR ごとの除外の設定: 設定の置き場と、除外を許す基準を新しく作ることになる。status で決まる規則なら、設定は要らない

これは ADR-0009 の「形の基準」の「必須は背景・検討した案・決定と理由、推奨は帰結・確認」の部分を置き換える。推奨の節は
`status: rejected` の ADR には求めない。ほかの部分は変わらない。

### 帰結

- 良い点: 番号だけを残した rejected の ADR が、毎回の warning を出さなくなる
- 悪い点: rejected の ADR で帰結を書き忘れても気づけない。xlc の運用（採らないと決めたら理由を「帰結」に短く書く）は、
  検査では確かめず、人が守る
- 実装: `RECOMMENDED_SECTIONS` を `status: rejected` の ADR に当てない

### 確認

- `modules/home/parts/design-records/src/check.rs` のテスト: rejected の ADR は帰結と確認が無くても warning を出さない。
  必須の節が無ければ error になる。proposed や accepted などの規則は変わらない
- xlc で `design-records check` を実行し、ADR-0091 と ADR-0092 の warning が出ない
