---
name: adr
description: ADR（docs/adr/）を書く・直すときに使う。MADR を基準にした形、フロントマター、確定後の運用。
allowed-tools:
  - Bash(design-records new:*)
  - Bash(design-records renumber:*)
  - Bash(design-records check:*)
  - Bash(design-records index:*)
---

# ADR

何をなぜ決めたかを、1 件 1 決定で残す。形は [MADR](https://adr.github.io/madr/) 4.0.0 に従い、違う点だけを下に書く。

自分のプロジェクトの ADR はこの形で書く（既存の ADR が別の形でも、新しい ADR から）。他人のプロジェクトでは、
そのプロジェクトの流儀に従う。

## ADR にするもの

作り直したときに他へ波及する決定（データの形式、モジュールの境界、外部への依存、検証の方法など）と、範囲、期間、危険、
費用のどれかが大きい決定。それ以外の決定は、issue かコミットの本文に書く。

試して捨てた方式は、何を試し、なぜ捨てたか（測った値や反例）を、関係する ADR の検討した案か補足、または issue に書く。

## 作る・検査する

形（フロントマターのキーと status、必須と推奨の節、番号）を持つのは CLI の `design-records` で、下の説明と食い違えば CLI が正。

```bash
design-records new <slug> "<決めたことの 1 文>"
design-records renumber <旧番号> [<新番号>]
design-records check [--changed] [FILE]...
design-records index
```

- `new`: `docs/adr/NNNN-<slug>.md` を、次の番号、フロントマター、必須と推奨の見出しつきで作る。slug は決めたことを動詞で
  始めた英語の kebab-case。番号は CLI が予約のタグ（`adr-NNNN`）を打って振り、origin があれば push する（origin に
  届かなければ、記録を作らずに止まる）。予約して使わなかった番号は欠番にする。docs/issues の issue の番号も
  `--kind issue` で振る（タグは `issue-NNNN`、表題の行だけを書く。フロントマターと節の形は各リポジトリの README に従う）
- `renumber`: rebase の後に `check` が番号の重複を示したときに、このブランチで足した記録の番号を振り直す。ファイル名、
  表題、このブランチで足した行の参照を付け替えて 1 つのコミットにするので、作業ツリーの変更（追跡していないファイルを
  含む）を先に片付ける。main から来た行の参照と、振り直す記録のフロントマターの参照は、main の記録を指すので
  そのまま残る。マージした後の予約のタグは `post-merge-cleanup.sh` が消す
- `index`: `docs/index.md` のフロントマターに `okf_version` を持つリポジトリ（`docs/` 全体が OKF のバンドル）で、索引に
  載っていない文書の行を足す。`new` で ADR を作ったら実行し、足した索引もコミットする。そのリポジトリでは `check` が
  `type`（ADR は `type: ADR`、`docs/issues/` の issue は `type: Issue`）、各ディレクトリの `index.md`（文書と下位のディレクトリがすべて載り、
  リンク先が実在すること）、`log.md` の日付の見出しも検査する
- `check`: ADR の形、ADR どうしの参照、ADR と docs/issues の番号の重複を確かめ、違反（error）があれば失敗する。
  注意（warning）では止まらないので、文言に従って確かめる。FILE を渡すと、置き換えられた ADR への参照と、確定した ADR の
  凍結する部分（下の「確定後の運用」）の変更も、FILE の分だけ注意する
  - 予約のタグの slug と記録の slug が違うという注意: 記録を改名したのなら、注意が示すコマンドでタグを新しい slug に
    打ち直す（`git push -f` では送らない。ほかの clone が先に動かした予約を奪う）。ほかの記録が番号を取ったのなら、
    打ち直さず `renumber` する（打ち直すと本来の持ち主の予約を奪う）

プロジェクトは `.pre-commit-config.yaml` に次を足してオプトインし、コミットのたびに検査する:

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
  補足。必須は背景・検討した案・決定と理由、推奨は帰結・確認（`rejected` の ADR には推奨の節を求めない。採らない理由を
  書く節はプロジェクトの運用に従う）。ほかの節を足してもよい
- **フロントマター**のキーは `type: ADR`（`new` が書く）、`status`、`date` と次の 4 つ。一覧は `[a, b]` の形で 1 行に書く
  - `requires`: 前提にしている ADR（`[ADR-NNNN]`）
  - `supersedes` / `superseded-by`: 置き換えた ADR と、置き換えられた ADR。両方の ADR に書く
  - `issues`: きっかけの issue と、解決した issue（`["#12"]`）
  - 既存の ADR に残る `amends` / `amended-by`（決定の一部を別の ADR で置き換えていたころの結び）は、新しくは書かず、
    既にあるものは消さない
- **status** は `proposed`、`accepted`、`rejected`、`withdrawn`（決める前に取り下げた）、`deferred`（後回しにした）、
  `superseded`、`deprecated`。`date` は最後に status が変わった日
- **帰結**: 実装が決定に追いついていなければ、その差を書き、issue を立てて `issues` に入れる
- **前の決定に戻す**とき: その理由と測った値を、新しい ADR の検討した案に書く
- **測った値や変わりうる値**には、出典のファイルとコミットと日付を添える
- 採用中の ADR の一覧は、ファイルにせず `grep -l '^status: accepted$' docs/adr/[0-9]*.md` で引く

## 確定後の運用

決めた時点の記録は凍結し、今の事実を書く節は直し続ける。決定の一部の変更は同じ ADR の中に書く（ADR を読む LLM は
ファイルの今の版だけを読むので、今の決定と前の決定を 1 件の中に残す）。

- **proposed と deferred の間**: 自由に直す。新しい ADR は、できるだけ実装と同じ PR に入れる。実装をマージするブランチで、
  帰結と確認を実装に合わせ（下の、マージ前の項）、`status: accepted` にする
- **確定した後**（accepted、rejected、withdrawn、superseded、deprecated）:
  - 凍結する部分: 背景、決定の要因、検討した案、決定と理由（決定と理由の下に置いた帰結と確認を除く）、各案の長所と短所、
    表題、フロントマターの `requires` / `supersedes`。直してよいのは、誤字、文法、マークアップ、壊れたリンク、
    日本語の辞書に合わせた、意味を変えない語の置き換えだけ
  - 直してよい部分: 帰結と確認（今の事実を書く節。マージの後の実装の進み、測った値、issue への結びをその場で書き直す）、
    補足の節への日付つきの追記（`- YYYY-MM-DD: …`）、末尾の「## 変更」、フロントマターの `status` / `superseded-by` /
    `date` / `issues`（帰結に書いた残りの issue）/ `amends` / `amended-by`
- **決定の一部を変える**: 新しい ADR を作らず、同じ ADR の決定と理由を今の決定に直し、末尾の「## 変更」の節（無ければ
  足す）に 1 項を足す。項の形は `- YYYY-MM-DD: <変えた部分>。前は「<前の文の引用>」。<理由>`。比べた案があれば、
  その項に書くか issue に書いて項から指す。`check` が決定と理由の変更に出す注意を、「## 変更」を書いたかを確かめる
  合図にする
- **置き換え**（決定の全体を変える）: 新しい ADR は、古い決定のうちまだ有効な部分を書き直して含める（採用中の ADR だけで
  今の決定が分かるように）。古い方は `status: superseded` と `superseded-by` を書き、末尾の「## 変更」に置き換えた日付と
  新しい ADR を書いてよい。古い方を前提にしていた ADR を `grep -l 'requires:.*ADR-NNNN' docs/adr` で引き、見直す
- 既存の ADR は元の形のまま残す（`amends` で結んだ古い一部の置き換えも）
- main にマージする前は、ブランチの上の ADR を status によらず直してよい。同じブランチで実装（コード、スキル、hook）を
  レビューや作業の途中で直し、ADR の決定・帰結・確認に書いた内容が変わるなら、そのコミットで ADR を合わせ、作り直す前の
  版と捨てた理由を検討した案か補足、または issue に書く。凍結する部分が実装と食い違ったまま accepted でマージしない
  （マージの後は「## 変更」が要り、誤った決定が決めた時点の記録として残る）。帰結と確認の食い違いは、マージの後でも
  直せる。直した実装が main の採用中の ADR の決定と食い違うなら、その ADR の決定と理由と「## 変更」を直すか
  （一部の変更）、置き換えの新しい ADR を書くか、/topic-triage で振り分ける
