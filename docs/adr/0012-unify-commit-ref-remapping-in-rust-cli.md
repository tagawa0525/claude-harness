---
status: accepted
date: 2026-10-11
requires: []
supersedes: []
superseded-by:
issues: ["#287"]
---

# ADR-0012: コミットの番号の付け直しを Rust の CLI commit-refs にまとめる

## 背景

履歴を書き換えると、文書（検証の記録、issue、ADR）に書いたコミットの番号が古くなる。付け直す道具は 2 か所にあった
（2026-10-11、nixfiles 05290ef）。

- `.claude/scripts/remap-commit-refs.sh`（bash と埋め込みの Python、159 行）: rebase と amend の後に git の
  post-rewrite hook から呼ばれ、作業ツリーの追跡している文書の番号を付け直す
- xlc の `references/history-rewrite`（Python と git filter-repo、228 行。git で追跡していない）: xlc の履歴を
  書き換えたとき（2026-10-11）に、履歴の中の各コミットの文書と本文の番号も付け直した

番号の照合（7 桁以上で単語の境界に挟まれた 16 進の語、先頭 7 桁の索引、曖昧な語と消えたコミットを指す語で止める）は、
両方が同じものを別々に持っていた（xlc の側は注釈で「remap-commit-refs.sh に従う」と書いて写していた）。履歴の中の
付け直しは、xlc の 1 回限りの道具にしか無く、xlc に固有の処理（用語の言い換え、msgpack の除去、表の揃え直し）と
混ざっていた。

## 決定の要因

- 照合の規則を 1 か所に置き、作業ツリーと履歴とで同じ語を同じように扱う
- 履歴の付け直しを、どのリポジトリでも使える形で残す（xlc に固有の処理は入れない）
- hook から呼ぶ道具と hook の版がずれない

## 検討した案

- 案 A: `remap-commit-refs.sh` をそのまま使い、履歴の付け直しを別のスクリプト（Python）で足す
- 案 B: 履歴の付け直しだけを Rust の新しい CLI にし、`remap-commit-refs.sh` は残す
- 案 C: 1 つの Rust の CLI `commit-refs` に、作業ツリーの付け直し（`remap`）と履歴の付け直し（`rewrite-history`）を持たせ、
  `remap-commit-refs.sh` を消す
- 案 D: `design-records` に入れる

## 決定と理由

採用: 案 C。

- 照合の規則が 1 か所になる。案 A と案 B では、照合が言語の違う 2 か所に分かれたまま残る
- 履歴の付け直しは、fast-export の流れを読みながら番号を置き換えるので、大きな履歴でもプロセスを繰り返し起こさずに
  済む言語で書く。nixfiles にはすでに Rust の CLI（claude-hooks、design-records）があり、作り方（`package.nix` の
  `buildRustPackage`、`tests/` の結合テスト）を揃えられる
- 外部の crate は使わない。読むのは行と 16 進の語だけで、標準ライブラリで足りる（design-records と同じ）
- 案 D は退ける。design-records は設計の記録の形を持つ CLI で、コミットの番号の付け直しとは扱うものが違う

hook からの呼び方:

- post-rewrite hook は `commit-refs` を Nix store のパスで呼ぶ。hook と CLI が同じ rebuild で決まり、版がずれない。
  `~/.claude` の同期を忘れてスクリプトが無い、という失敗が無くなる
- 手で呼ぶとき（filter-repo の後、hook が失敗した後）のために、home-manager で PATH にも入れる

`remap` の振る舞いは `remap-commit-refs.sh` と同じにする（対応表の 2 つの形、置き換えの規則、止める条件、出力の
`REPLACED:` / `FILES:` / `REFS:`）。xlc に固有の処理は移さない。次だけを変える:

- 旧と新の桁数が違う行（SHA-1 と SHA-256 の混在など）は、対応表の誤りとして止める。スクリプトは新を短く切り、
  文書の語より短い番号を黙って書いていた。0 だけの印（消えたコミット）は置き換えに使わないので、桁数を問わない
- 書き換える前に止めたときは、どの理由でも「何も書き換えていない」と出す。書き込みの途中で失敗したときは、
  書き換え済みのファイルを出し、失敗したファイルは元のまま残す（一時ファイルに書いて rename で置き換える）。
  スクリプトは前者の一部でしかそう出さず、書き込みに失敗したファイルを書きかけで残すことがあり、hook はどの
  失敗でも「何も書き換えていません」と出していた
- 変わらなかったコミット（旧と新が同じ行）も照合に使い、短い語がそれと変わったコミットの両方に当たれば曖昧として
  止める。スクリプトは、変わったほうに黙って置き換えていた
- 新が別の行の旧として変わる連鎖（A → B と B → C）は、対応表の誤りとして止める。1 回の置き換えでは A が C に
  ならず、書き込みの途中で失敗した後に呼び直すと、書き換え済みの語をもう一度書き換えるため。post-rewrite の入力と
  filter-repo の commit-map には連鎖が出ない
- git grep が読めなかったファイルも止める理由にする。スクリプトは、git grep が標準エラーに出すだけのこの
  ファイルを、黙って付け直さずにいた
- 使い方の誤り（引数が 2 つ以上、`-` で始まる引数）の終了コードを 1 から 2 にし、design-records と揃える。
  hook は引数を渡さないので影響しない
- hook が失敗したときに対応を残すファイルを、`remap-commit-refs.input` から `commit-refs-remap.input` にする

### 帰結

- 良い点: 照合の規則が 1 か所になり、作業ツリーと履歴の付け直しが同じテストで守られる
- 良い点: hook が呼ぶ道具の有無を確かめる分岐（`~/.claude/scripts` に無いとき）が要らなくなる
- 悪い点: 振る舞いを変えるには Rust を直してビルドする。スクリプトのように、その場で直して試せない
- 実装が決定に追いついていない点: `rewrite-history` は別の PR で足す（#287）。どう書き換えるかは別の ADR で決める

### 確認

- `commit-refs remap` の結合テスト（`modules/home/parts/commit-refs/tests/remap.rs`）が、`remap-commit-refs.sh` の
  テストの全ての場合を引き継ぎ、シンボリックリンク、64 桁を超える語、大文字、ASCII でない文字の境界を足す
- 移す前に、xlc の clone（2026-10-11、xlc f828bf78）で 400 コミットの対応表を `remap-commit-refs.sh` と
  `commit-refs remap` に渡し、出力と作業ツリーが一致した（置き換えるだけの場合は 28 ファイル 130 語、消えたコミットを
  含む場合は同じ 9 件の ERROR で止まった）
- post-rewrite hook のテスト（`modules/home/parts/tests/post-rewrite-remap.sh`）が、ビルドした `commit-refs` を呼ぶ
  hook で通る

## 補足

- 2026-10-11: 帰結の「実装が決定に追いついていない点」の `rewrite-history` を足した（ADR-0013）
