---
status: proposed
date: 2026-11-17
requires: [ADR-0009]
supersedes: []
superseded-by:
issues: []
---

# ADR-0092: docs/ を OKF のバンドルとして design-records check で検査する

## 背景

OKF（Open Knowledge Format）v0.2 は、マークダウンと YAML のフロントマターだけで知識を表す形式で、概念の文書に `type` を
必須とし、各ディレクトリの `index.md` で内容を一覧にする（SPEC §3、§4.1、§8）。NC リポジトリの `docs/` はこの形で書いている
（仕様の原典は NC の `references/OKF/SPEC.md`）。このリポジトリの `docs/`（`adr/` の 76 件と `windows.md`）には `type` も
`index.md` も無く、将来の `docs/issues/` や `docs/plans/` を同じ形で書く決まりも無い。

ADR と issue の形を検査するのは CLI の `design-records`（ADR-0009）で、プロジェクトの pre-commit が呼ぶ。OKF の形も
ここで検査すれば、同じ hook で違反を止められる。

ADR の `status` は `proposed`、`accepted` など 7 値で、OKF の `status`（`draft`、`stable`、`deprecated`）と値が違う。
OKF は未知のキーと値を許す（SPEC §11）。

## 決定の要因

- `type` と `index.md` が揃っていないことを、コミットのたびに機械的に見つける
- 自分以外のプロジェクトや、OKF を使わないプロジェクトの `design-records check` を壊さない
- 既存の ADR の意味（`status` の値）を変えない
- 新しい文書を足すときの手作業を増やさない

## 検討した案

- 案 1: `design-records check` に OKF の検査を足す。`docs/index.md` のフロントマターに `okf_version` を持つリポジトリだけを
  対象にする（オプトイン）
- 案 2: OKF の検査だけをする別の CLI を作る
- 案 3: 案 1 のうえで、ADR の `status` を OKF の値に置き換える（`accepted` → `stable` など）

## 決定と理由

案 1 を採る。

- `docs/index.md` に `okf_version` があるリポジトリだけを対象にする。ほかのリポジトリの `docs/` は今までどおり見ない
- 対象の `docs/` 以下の、追跡している `.md` 全部を 1 つのバンドルとして、次を違反（error）にする
  - `index.md` と `log.md` 以外の文書は、フロントマターを読め、空でない `type` を持つ
  - ADR は `type: ADR`、`docs/issues/` の issue は `type: Issue`
  - 文書か下位のディレクトリを持つディレクトリに `index.md` がある
  - `index.md` のフロントマターは、バンドルのルートの `okf_version` だけ
  - `index.md` の `* [題](パス) - 説明` の行のリンク先が実在し、同じディレクトリの文書と下位のディレクトリがすべて載っている
  - `log.md` の日付の見出しが `YYYY-MM-DD`
- 索引の題と説明が、リンク先のフロントマターの `title` と `description` と違うときは注意（warning）にする。ADR は
  フロントマターに `title` を持たないので、題は最初の 1 段目の見出しと比べる
- `design-records index` が、索引に載っていない文書と下位のディレクトリの行を足す（説明は文書の `description` から取る。
  既にある行は直さない）。`index.md` が無ければ作る
- `design-records new` が作る ADR のテンプレートに `type: ADR` を足す。`type` は ADR の任意のキーで、確定した後も直して
  よいキーにする（既存の ADR に足すため）
- ADR の `status` は今の 7 値のままにする。OKF の `status` は文書の鮮度の合図で、ADR の `status` は決定の進み具合を表す別の
  もの。OKF は未知の値を許し、置き換えると既存の 76 件と、`status` を読むスキルとスクリプトが全部変わる

案 2 は、ADR のパスと中身を読む処理とフロントマターの読み取りをもう 1 つの CLI に持たせ、pre-commit の hook も 2 つになる。
案 3 は上の理由で、得るものに比べて波及が大きい。

### 帰結

- 既存の ADR 76 件に `type: ADR` を足す 1 つのコミットが要る（`type` は凍結する部分ではないので、`AMENDABLE_KEYS` に足す）
- `docs/issues/` の新しい issue には、手で `type: Issue` を足す（`design-records new --kind issue` の下書きは表題の行だけのまま。
  フロントマターの形は各リポジトリが決める、ADR-0018）

### 確認

- `modules/home/parts/design-records/src/okf.rs` の単体テストと `tests/cli.rs` の `check` と `index` の試験
- このリポジトリの `docs/` で `design-records check` が違反なしで通る

## 各案の長所と短所

- 案 1: 長所は、pre-commit の設定を変えずに効き、オプトインで他を壊さない。短所は、`design-records` の責務が ADR と issue の
  ほかに広がる
- 案 2: 長所は、責務が分かれる。短所は上のとおり、読み取りの重複と hook の追加
- 案 3: 長所は、OKF の値と揃う。短所は、既存の記録とスキルの全面的な書き換え
