---
status: accepted
date: 2026-10-29
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0064: /git-merge はマージの前に、実装した proposed の ADR を一覧に出し、止めない

## 背景

xlc の docs/adr/README.md は、実装をマージするブランチで proposed の ADR を実装に合わせて直し、同じ変更で accepted に
すると決めている。それでも、実装をマージしたのに proposed のまま残った ADR が 6 件あった（xlc の
docs/issues/0188。ADR-0144、0151、0152、0169、0202、0206）。/git-merge の手順には ADR の status を見る段が無く、
マージする親のセッションが見落とした。accepted になった ADR はマージの後に本文を直せないので、気づくのは
マージの前でなければならない。

xlc のコミットは、本文の `Refs:` の行で ADR を挙げる（`Refs: ADR-0202`、`Refs: #0133, ADR-0202`。xlc の main の
直近 400 コミットの `Refs:` の行はすべてこの形）。実装のコミットの件名は Conventional Commits の feat か fix で、
xlc の #0188 の 6 件は `git log --grep="Refs:.*ADR-NNNN" main | grep -E ' (feat|fix)\('` で数えた。

2026-10-29 の 14 回目の /retro で、提案 2「/git-merge に、ブランチの feat/fix のコミットが `Refs:` で挙げた proposed の
ADR を一覧に出す 1 段。hook で止めない」をユーザーが選んだ（「推奨に従います」）。

## 検討した案

- 案 A: hook でマージを止める（feat か fix のコミットが挙げた ADR が proposed なら拒む）。一部だけを実装した
  ブランチ、方法を試すブランチ、決定の一部を後のブランチに残すブランチは、ADR を proposed のまま残してよい。
  どれに当たるかは ADR の本文と実装を読まないと決められず、機械で判定すると誤って止める。止めた後の逃げ道
  （エスケープ）を足すと、それが手順の一部として使われる
- 案 B（採用）: マージの前の状態の一覧に出し、スキルの 1 段で ADR ごとに決める。判定はしないので誤って止めない。
  見落としは、一覧が `none` でない限りスキルの段が読ませる
- 案 C: 何もしない（README の規則だけに任せる）。6 件の見落としがこの形で起きた

一覧を出す場所:

- `git-merge-state.sh` に `PROPOSED_ADRS:` の行として足す（採用）。対象のブランチと既定ブランチを既に決めていて、
  比べる範囲（`<既定>..<対象>`）がそのまま使える。スキルの動的な行は 1 本のままで、出力の形（`KEY: value`）も揃う
- 別のスクリプトを足し、スキルに動的な行を 1 本足す。対象と既定ブランチの決め方を複製する。GitHub の流れ
  （/gh-pr-merge）で使う予定は今は無い
- `design-records` のサブコマンドにする。git の範囲、件名の型、`Refs:` の解釈は設計の記録の形の外で、CLI が
  git の履歴の規約まで持つことになる。status を読むところだけは `design-records status`（標準入力の文書の
  フロントマターの status を出す）を使い、フロントマターの形の解釈を 1 か所に保つ

## 決定と理由

案 B を採り、`git-merge-state.sh` に次の行を足す:

```text
PROPOSED_ADRS: ADR-NNNN ...|none
```

対象は、`<既定>..<対象>`（既定ブランチが無ければ対象の全コミット）のうち、件名の型が feat か fix（scope と `!` を
含む）のコミットの本文の `Refs:` の行にカンマで区切って挙がる値のうち `ADR-NNNN` そのもの（「nixfiles の ADR-0036」の
ような別のリポジトリの参照は数えない）で、対象の先端で `docs/adr/NNNN-*.md` の status が
proposed のもの。番号の順に重複なく出す。対象の先端に docs/adr が無いリポジトリでは行を出さない。

feat と fix に絞るのは、docs のコミット（ADR を書く、直す）や test のコミットが ADR を挙げても、実装を含むとは
限らないため。status をブランチの先端で読むのは、同じブランチで accepted にしたものを一覧から外すため。

/git-merge の「proposed の ADR」の段は、ADR ごとに、全部を実装したなら直して accepted にし、一部なら残りを帰結と
issue に書いて accepted にするか proposed のまま残し、試しただけなら proposed のまま残す、と決めさせる。
proposed のまま残したものは、マージの本文に番号と理由を書く。

### 帰結

- `git-merge-state.sh` は、`Refs:` の番号に一致する ADR のファイルがあるとき `design-records` を呼ぶ。PATH に
  無ければ失敗する（黙って一覧を省かない）。一致するファイルが無い番号（打ち間違い、まだ書いていない ADR）は一覧に
  出ない
- 一覧は、読んだときのブランチの先端のもの。rebase の後は再実行する。rebase の後に `design-records renumber` で番号を
  振り直した ADR は、前のコミットの `Refs:` が古い番号を指したまま残るので、一覧からは分からない（コミットの
  メッセージは振り直しで書き換わらない）
- 一覧は feat と fix のコミットの `Refs:` に頼る。実装のコミットが ADR を挙げ忘れれば一覧に出ない。挙げた ADR の
  決定の一部だけを実装したかは、一覧からは分からず、段で読んで決める
- GitHub のリモートのあるリポジトリ（/gh-pr-merge の流れ）には足していない。そちらで同じ見落としが起きたら、
  同じ行を出す場所を決め直す

### 確認

- `.claude/tests/scripts/git-merge-state.sh` の `PROPOSED_ADRS` のテスト（feat、fix、feat! が挙げた proposed、
  サブディレクトリからの実行、log.showSignature と署名したコミット、accepted、docs と test と件名の途中の `feat:`、
  `Refs:` の行の外と別のリポジトリの ADR、main に入ったコミット、main の無いリポジトリ、docs/adr の無いリポジトリ）が通る
- xlc の #0188 の ADR-0169、0202、0206 の実装をマージしたコミット（316e30b6a、cc26dc53c、b9d38a653）の第 1 親から
  第 2 親までに同じ判定を当てると、それぞれの ADR が一覧に出る（2026-10-29 に確かめた）
