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
  スキルの段は、rebase の後に状態を取り直させ、一覧の ADR（と振り直した ADR）ごとに決めさせる
- 案 C: 何もしない（README の規則だけに任せる）。6 件の見落としがこの形で起きた

コミットの区切り方: 最初は `git log --format='%s%n%b%x00'` を awk の `RS = "\0"` で読んだ。RS に NUL を渡す
書き方は awk の実装で動きが違い、BWK awk（macOS の /usr/bin/awk）は RS を空として段落モードで読むので、
`Refs:` の段落が件名から切り離されて黙って一致しない（レビューの指摘）。`git log -z --format=%B` を bash の
`read -d ''` で読む形に替えた。

レビューで作り直したほかの版（どれもレビューの指摘で、反例をテストにした）:

- docs/adr の有無を `git cat-file -e` で見た版は、docs/adr がファイルでも通り、後の ls-tree が `ERROR:` の行なしに
  失敗した。`cat-file -t` の stderr を捨てた版と `rev-parse --verify -q` の版は、木を読めない（壊れたリポジトリ、
  部分 clone）ときも「docs/adr が無い」になり、行を出さずに 0 で終わった。`git ls-tree --full-tree <先端> docs/adr`
  （無ければ空、読めなければ失敗）にした
- ブランチを短い名前で指した版は、同じ名前のタグがあるとタグのコミットを読んだ。refs/heads/ で指す
- design-records を docs/adr があるだけで求めた版、ADR の参照があるだけで求めた版は、読む status が無いのに
  止まった。番号の一致するファイルを読む直前で求める
- Refs: の値を `{ grep … || true; }` で絞った版は、grep の失敗（終了コード 2）も空の一覧にして none を出した。
  一致なし（1）だけを通す
- `%s%n%b` で読んだ版は、件名の直後に空行なしで書いた `Refs:` を件名に繋げて落とした。`%B` で読む
- `Refs:` の行の `ADR-[0-9]{4}` をすべて拾った版は、「nixfiles の ADR-0036」のような別のリポジトリの参照を数えた。
  カンマで区切った値が `ADR-NNNN` そのものであるものだけを数える

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
issue に書いて accepted にするか proposed のまま残し、試して採らないと決めたなら rejected にし、試しただけなら
proposed のまま残す、と決めさせる（accepted にするときは「確認」に実際のテストと結果を書く）。proposed のまま残したものは、
マージの本文に番号と理由を書く。段は、rebase の後に状態を取り直してから決めさせる（rebase で一覧が増えも減りもする）。

### 帰結

- `git-merge-state.sh` は、feat か fix のコミットが挙げた番号の ADR のファイルを読むとき `design-records` を使う。PATH に無ければ `ERROR:` を出して
  止まる（行が無いことを docs/adr の無いリポジトリと取り違えさせない）。一致するファイルが無い番号（打ち間違い、
  まだ書いていない ADR）は一覧に出ない
- perf、refactor、build などの型のコミットだけが実装した ADR は一覧に出ない（xlc の main には、`Refs:` で ADR を
  挙げた perf、refactor、build のコミットもある）。そうした漏れが起きたら、型を広げるかを決め直す
- 一覧は、読んだときのブランチの先端のもの。rebase の後は再実行する。rebase の後に `design-records renumber` で番号を
  振り直した ADR は、前のコミットの `Refs:` が古い番号を指したまま残る（コミットのメッセージは振り直しで書き換わらない）。
  振り直した ADR は一覧から漏れ、古い番号の main の別の ADR が proposed なら一覧に出る。スキルの段は、一覧の ADR が
  このブランチの実装したものかを本文で確かめさせる
- スキルの段は、チェックとレビューの前（番号の重複の確認の後）に置く。直した ADR を同じチェックとレビューで見るため
- 一覧は feat と fix のコミットの `Refs:` に頼る。実装のコミットが ADR を挙げ忘れれば一覧に出ない。挙げた ADR の
  決定の一部だけを実装したかは、一覧からは分からず、段で読んで決める
- GitHub のリモートのあるリポジトリ（/gh-pr-merge の流れ）には足していない。そちらで同じ見落としが起きたら、
  同じ行を出す場所を決め直す

### 確認

- `.claude/tests/scripts/git-merge-state.sh` の `PROPOSED_ADRS` のテスト（feat、fix、feat!、fix! が挙げた proposed、1 つのコミットの複数の `Refs:` の行、件名の直後の空行の無い `Refs:` の行、
  サブディレクトリからの実行、log.showSignature と署名したコミット、accepted と superseded、docs と test と件名の途中の `feat:`、
  `Refs:` の行の外と別のリポジトリの ADR、main に入ったコミット、main の無いリポジトリ、ファイルの無い番号、design-records が PATH に無いとき（ADR の参照が無ければ続行する）、
  ブランチや main と同じ名前のタグ、docs/adr がファイルのとき、docs/adr の木を読めないとき、grep が失敗したとき、docs/adr の無いリポジトリ）が通る
- xlc の #0188 の ADR-0169、0202、0206 の実装をマージしたコミット（316e30b6a、cc26dc53c、b9d38a653）の第 1 親から
  第 2 親までに同じ判定を当てると、それぞれの ADR が一覧に出る（2026-10-29 に確かめた）
