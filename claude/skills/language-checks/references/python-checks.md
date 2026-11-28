# Python Quality Checks

上から順に実行する。どれも終了コード 0 で成功。

## 1. Format Check (ruff format)

```bash
ruff format --check .   # 検査
ruff format .           # 失敗時の自動修正
```

## 2. Lint Check (ruff check)

```bash
ruff check .         # 検査
ruff check --fix .   # 失敗時の自動修正
```

## 3. Test (pytest)

```bash
pytest
```

自動修正は無い。失敗の詳細を確認して直す。

run-checks.sh は、テストを次の順に探し、最初に当たったものを実行する。

1. `scripts/run-python-tests.sh` があれば、それを実行する（終了コードで判定。実行権限が無ければ失敗）
2. `tests/` かルート直下の `test_*.py` があれば、`pytest` で実行する
3. git が追跡している `test_*.py` / `*_test.py` がサブディレクトリにあれば、そのディレクトリごとに、追跡している
   ファイルだけを渡して実行する。インタープリタは `python3`、`python` の順に `--version` が通る方を使い（Windows の
   Git Bash の `python3` は Microsoft Store のスタブで、何を渡しても失敗する）、`<インタープリタ> -m pytest` が使えれば
   pytest、無ければ `<インタープリタ> -m unittest <モジュール>`。どちらも動かなければ SKIP
4. テストが 1 つも無いときだけ SKIP

手で流すときも同じく選ぶ。Windows では `python3` でなく `python -m pytest` と書く。
