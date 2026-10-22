---
status: accepted
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
ファイルとして残したもので、どちらも帰結と確認の節を持たず、採らない理由と記録した先を「決定と理由」に書いている。
確定した後は本文を変えられないので、ファイルを指定せずに `design-records check` を実行するたびに、この 2 件の 4 つの
warning が並ぶ（xlc、682dead38、2026-10-22）。pre-commit と run-checks.sh は `check --changed` で渡したファイルの
warning だけを出すので、この 2 件を触らないコミットでは出ない。

- 確定後の ADR の本文は変えられない（ADR-0009）ので、warning を消すために書き足せない
- 採らなかった決定には、確かめる実装も、決定が起こす帰結も無い
- 新しい rejected の ADR の書き方（xlc の README の運用: proposed の ADR を採らないと決めたら、理由を「帰結」に書く）は
  変えない

## 検討した案

- 案 A: rejected の ADR には、帰結と確認のどちらも求めない
- 案 B: rejected の ADR には、確認だけを求めない。帰結は求める
- 案 C: 確定した ADR に節を書き足すことを許す
- 案 D: ADR ごとの除外の設定で warning を消す
- 案 E: 推奨の節の warning を、本文を直せる ADR（proposed、deferred）だけに出す

## 決定と理由

採用: 「案 A」。

- 必須の節（背景、検討した案、決定と理由）は rejected でも求め、無ければ error にする。採らない理由をどの節に書くかは、
  各プロジェクトの運用に従う（xlc は「帰結」）。検査はそれを確かめない
- 新しい rejected の ADR は、`design-records new` のテンプレートで作った proposed の ADR から変わるので、帰結と確認の
  見出しをもう持つ。検査が見るのは見出しの有無だけで、理由を書いたかは見ないので、帰結の warning を残しても、新しい
  ADR の書き忘れはほとんど捕まらない。見出しを消してから rejected にした ADR だけが捕まらなくなる
- 「確認」は決定が成り立つことを示す節なので、採らなかった決定には対象が無い
- 他の status の規則は変えない。withdrawn も効力を持たずに終わった ADR で、同じ理由が当てはまりうるが、節の無い
  withdrawn の ADR は今どのプロジェクトにも無い（xlc、nixfiles、lsp-det、pleasanter-rs、2026-10-22）。必要になったら、
  その ADR で決める

採らなかった案:

- 案 B: xlc の ADR-0091 と ADR-0092 は帰結の節を持たない。確定後は直せないので、帰結の warning が残り、目標を満たさない
- 案 C: 確定した ADR は変えられないという規則（ADR-0009）の例外が増える。warning を消すためだけに、本文に中身の無い
  節を足すことになる
- 案 D: 設定の置き場と、除外を許す基準を新しく作ることになる。status で決まる規則なら、設定は要らない
- 案 E: 確定した古い ADR の warning も消えるが、proposed の ADR を実装のブランチで accepted にしたとき（帰結と確認を
  実装に合わせる時）に、節の無いことを示せなくなる。マージの前はブランチの上で直せるので、そこが一番 warning の
  効く時である

これは ADR-0009 の「形の基準」の「必須は背景・検討した案・決定と理由、推奨は帰結・確認」の部分を置き換える。推奨の節は
`status: rejected` の ADR には求めない。ほかの部分は変わらない。

### 帰結

- 良い点: 番号だけを残した rejected の ADR が、`design-records check` の warning を出さなくなる
- 悪い点: 帰結の見出しを消して rejected にした ADR に気づけない。xlc の運用（採らないと決めたら理由を「帰結」に書く）は、
  テンプレートの見出しと人の確認で守る
- 実装: 推奨の節は `recommended_sections(status)`（`modules/home/parts/design-records/src/lib.rs`）が status から選ぶ

### 確認

- `modules/home/parts/design-records/src/check.rs` のテスト: rejected の ADR は帰結と確認が無くても warning を出さない
  （`rejected_adrs_need_no_recommended_sections`）。必須の節が無ければ error になる（`rejected_adrs_still_need_required_sections`）。
  proposed、accepted、withdrawn、deferred、superseded、deprecated は、今までどおり warning を出す
  （`other_statuses_still_get_recommended_section_warnings`）
- このブランチで `nix build .#design-records` した CLI を xlc（682dead38）で `design-records check` として実行し、
  出力が空になった。main の CLI では ADR-0091 と ADR-0092 の帰結と確認の 4 つの warning が出る（2026-10-22）
