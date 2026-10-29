---
name: adr
description: ADR（docs/adr/）を書く・直すときに使う。MADR を基準にした形、フロントマター、確定後の運用。
allowed-tools:
  - Bash(design-records new:*)
  - Bash(design-records renumber:*)
  - Bash(design-records check:*)
---

# ADR

何をなぜ決めたかを、1 件 1 決定で残す。形は [MADR](https://adr.github.io/madr/) 4.0.0 に従い、MADR と違う点だけを
下に書く。

自分のプロジェクトの ADR はこの形で書く。他人のプロジェクトでは、そのプロジェクトの流儀に従う。既存の ADR が別の形の
自分のプロジェクトでは、新しい ADR からこの形で書く。

## ADR にするもの

作り直したときに他へ波及する決定（データの形式、モジュールの境界、外部への依存、検証の方法など）と、範囲、期間、危険、
費用のどれかが大きい決定。それ以外の決定は、issue かコミットの本文に書く。

試して捨てた方式は、何を試し、なぜ捨てたか（測った値や反例）を、関係する ADR の検討した案か補足、または issue に書く。
コミットの本文だけに置くと、後から引けない。

## 作る・検査する

形（フロントマターのキーと status、必須と推奨の節、番号）を持つのは CLI の `design-records` で、下の説明と食い違えば CLI が正。

```bash
design-records new <slug> "<決めたことの 1 文>"
design-records renumber <旧番号> [<新番号>]
design-records check [--changed] [FILE]...
```

`new` は `docs/adr/NNNN-<slug>.md` を、次の番号、フロントマター、必須と推奨の見出しつきで作る。slug は決めたことを動詞で
始めた英語の kebab-case にする。番号は、全ブランチの履歴と予約のタグ（`adr-NNNN`）のうち最大の番号 + 1 で、
注釈つきのタグを打って予約する。リモート origin があれば、origin のブランチとタグを fetch してから振り（予約を解放した番号は origin の既定ブランチの
履歴にしか残らない）、push する。
push が拒まれたら（ほかの clone が先に予約した）次の番号で振り直す。origin に届かなければ、記録を作らずに止まる。
予約だけして使わなかった番号は欠番にする（再利用しない）。

docs/issues の issue も同じ仕組みで番号を振る（`--kind issue`。タグは `issue-NNNN`、表題の行だけを書く。フロントマターと
節の形は各リポジトリの README に従う）。

`renumber` は、このブランチで足した記録（既定ブランチとの分岐点に無いもの）の番号を振り直す。rebase の後に
`check` が番号の重複を示したときに使う。ファイル名と表題を変え、このブランチで足した行の参照（`ADR-NNNN`、
`NNNN-<slug>.md`）を付け替えて、1 つのコミットにする。main から来た行の参照と、振り直す記録のフロントマターの参照は、
main の記録を指すのでそのまま残す。作業ツリーに変更（追跡していないファイルを含む）があれば止まる。マージした後の予約のタグは、`post-merge-cleanup.sh` が
消す。

`check` は docs/adr の追跡している ADR の形と、ADR どうしの参照（置き換えの双方向の一致、参照先の有無）、ADR と
docs/issues の番号の重複を確かめ、違反（error）があれば失敗する。予約のタグの slug と違う slug の記録には注意を出す。
タグの slug のファイルが無いときは改名かもしれない。改名なら、`prune` が slug の一致する予約しか消さないので、タグを新しい slug で打ち直す（`check` の注意が示すコマンドを実行する。`origin` があれば、打つ前のタグを期待値にした `--force-with-lease` で送り、拒まれたら手元のタグを戻す。`-f` の push は、GitHub のリモートでは hook が止め、ほかの clone が先に動かした予約も奪う）。ほかの記録が番号を取ったなら打ち直さず `renumber` する（打ち直すと本来の持ち主の予約を奪う）。
FILE を渡すと、そこから置き換えられた ADR への参照と、確定した ADR の本文の変更を探し、
FILE の分だけ注意（warning）を出す（`--changed` なら FILE が無くても warning を出さない）。プロジェクトは
`.pre-commit-config.yaml` に次を足してオプトインし、コミットのたびに検査する:

```yaml
repos:
  - repo: local
    hooks:
      - id: design-records
        name: design-records
        entry: design-records check --changed
        language: system
        always_run: true # ADR を消すだけのコミット（渡すファイルが無い）でも検査する
        require_serial: true # ADR どうしの検査を 1 回で（分けて呼ぶと同じ指摘が重なる）
        types: [text]
        verbose: true # 通ったときも warning を見せる
```

## MADR と違う点

- **見出しは日本語**（英語の原文でもよい）: 背景、決定の要因、検討した案、決定と理由、帰結、確認、各案の長所と短所、
  補足。必須は背景・検討した案・決定と理由、推奨は帰結・確認（`rejected` の ADR には推奨の節を求めない。採らなかった
  決定には確かめるものも帰結も無く、節の無いまま確定した ADR は後から書き足せないため。採らない理由を書く節は
  プロジェクトの運用に従う）。
  ほかの節を足してもよい
- **フロントマター**のキーは `status`、`date` と次の 4 つ、それに書いてもよい 2 つ（MADR の `decision-makers` などに
  代えて、置き換えと参照を grep で引けるようにする）。一覧は `[a, b]` の形で 1 行に書く
  - `requires`: 前提にしている ADR（`[ADR-NNNN]`）
  - `supersedes` / `superseded-by`: 置き換えた ADR と、置き換えられた ADR。両方の ADR に書く
  - `issues`: きっかけの issue と、解決した issue（`["#12"]`）
  - 書いてもよいキー `amends` / `amended-by`: 決定の一部だけを置き換えた ADR と、一部を置き換えられた ADR
    （`[ADR-NNNN]`）。両方の ADR に書く。一部の置き換えはまれなので、すべての ADR には求めない
- **status** は `proposed`、`accepted`、`rejected`、`withdrawn`（決める前に取り下げた）、`deferred`（後回しにした）、
  `superseded`、`deprecated`。`date` は最後に status が変わった日
- **帰結**: 実装が決定に追いついていなければ、その差を書き、issue を立てて `issues` に入れる
- **前の決定に戻す**とき: その理由と測った値を、新しい ADR の検討した案に書く
- **測った値や変わりうる値**には、出典のファイルとコミットと日付を添える
- 採用中の ADR の一覧は、ファイルにせず `grep -l '^status: accepted$' docs/adr/[0-9]*.md` で引く

## 確定後の運用

決定を変えるときは新しい ADR で置き換え、ADR は増えていく前提にする。

- **proposed と deferred の間**: 自由に直す。実装をマージするブランチで、帰結と確認を実装に合わせ（下の、マージ前の項）、`status: accepted` にする
- **確定した後**（accepted、rejected、withdrawn、superseded、deprecated）に直してよいのは、誤字、文法、マークアップ、
  壊れたリンク、フロントマターの `status` / `superseded-by` / `date` / `amends` / `amended-by`、補足の節への日付つきの追記（`- YYYY-MM-DD: …`）、日本語の辞書に合わせた、意味を変えない語の置き換えだけ
- **置き換え**: 新しい ADR は、古い決定のうちまだ有効な部分を書き直して含める（採用中の ADR だけで今の決定が分かる
  ように）。古い方は `status: superseded` と `superseded-by` を書く。古い方を前提にしていた ADR を
  `grep -l 'requires:.*ADR-NNNN' docs/adr` で引き、見直す
- **一部の置き換え**: 古い決定の一部だけを変える ADR は、本文に「これは ADR-NNNN の「…」の部分を置き換える。ほかの
  部分は変わらない」と古い ADR の語で書き、`amends` に古い ADR を挙げる。古い方は status を変えず、新しい ADR が
  効力を持ったら（accepted にしたら）`amended-by` に新しい ADR を足す。`check` が両方向の一致を確かめる
- main にマージする前は、ブランチの上の ADR を status によらず直してよい。同じブランチで実装（コード、スキル、hook）を
  レビューや作業の途中で直し、ADR の決定・帰結・確認に書いた内容が変わるなら、そのコミットで ADR を合わせ、作り直す前の
  版と捨てた理由を検討した案か補足、または issue に書く。
  確定した ADR はマージの後に直せないので、食い違ったまま accepted でマージすると、誤った決定が残る（2026-10-24 に、
  スキルをレビューで作り直したのに ADR の決定の案が前の版のまま accepted で、PR のレビューで見つかって 2 周を足した）。
  直した実装が main の採用中の ADR と食い違うなら、置き換えか一部の置き換えの新しい ADR を書くか、/topic-triage で
  振り分ける（採用中の ADR の本文は上の「確定した後」の範囲でだけ直す）
