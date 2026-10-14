---
status: accepted
date: 2026-10-15
requires: []
supersedes: []
superseded-by:
issues: []
---

# ADR-0036: run-checks が Python のテストをサブディレクトリでも走らせる

## 背景

`run-checks.sh` の Python のテストの段は、`tests/` かリポジトリの直下の `test_*.py` / `*_test.py` があるときだけ pytest を
走らせ、無ければ `SKIP` で成功を返していた。xlc のテストは `examples/` と `examples/vera/` の下にあり（隣の .py を import する
道具のテスト）、チェックで一度も走らないまま通っていた（2026-10-15 に判明）。xlc の 1 つのテストは OpenMC の Python を import
するので、既定の devShell では落ちる。

## 検討した案

- 案 A（採用）: 既存の判定に該当しなければ、git が追跡している `test_*.py` / `*_test.py` をディレクトリごとに走らせる。
  リポジトリに `scripts/run-python-tests.sh` があればそれを最優先で走らせ、終了コードで判定する
- 案 B: run-checks 全体をリポジトリ固有の devShell で走らせる。nixfiles がリポジトリ固有の shell の名前を知ることになり、汎用で
  なくなる
- 案 C: テストを `tests/` に移す。テストが道具の隣にある配置（import の都合）を崩し、リポジトリごとに移す手間が要る

## 決定と理由

案 A。汎用の検出（追跡しているファイルだけを見るので、無視したディレクトリや `references/` は入らない）は nixfiles に置き、
走らせ方の違い（devShell を分けるなど）はリポジトリの `scripts/run-python-tests.sh` に任せる。ディレクトリごとに
走らせるのは、テストが隣の .py を import するため。pytest が `python3 -m pytest` で使えればそのディレクトリの追跡ファイルだけを
渡し（子のディレクトリのテストを重ねて走らせない）、無ければ標準の unittest で走らせる（pytest を足さない環境でテストが
走らないまま通るのを避ける）。

### 帰結

- Python の段は .py の変更があるときだけ走る（既存の検出のまま）。全部を走らせたいときは `scripts/run-python-tests.sh` を
  手で走らせる
- pytest 形式（関数だけ）のテストは、python3 から pytest が使えない環境では unittest がテストを 1 件も見つけず、
  exit 5（NO TESTS RAN）でこの段が落ちる。pytest を devShell に足すか、override を置く
- ルート直下と `tests/` の判定は従来どおり `pytest` コマンドで、サブディレクトリは `python3 -m pytest` で見る。前者は
  既存の挙動を変えないため、後者は xlc で PATH の pytest が devShell の python3 と別の環境だったため
- 既存のルート直下の判定（`compgen -G`）は、PATH 先頭の bash に compgen が無い環境で偽になる（#375。この ADR では直さない）

### 確認

- `.claude/tests/scripts/language-checks.sh` に、サブディレクトリのテストが落ちると `FAILED: python test` になること、pytest の
  無い環境で unittest に落ちること、override が優先されることの回帰テストを足した（122 件通過）
- xlc（`scripts/run-python-tests.sh` で references の devShell）で 4 つのテスト（35 件）が run-checks で走って通った
