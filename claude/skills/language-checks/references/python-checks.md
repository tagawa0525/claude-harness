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
   ファイルだけを渡して実行する。インタープリタは、`python3`、`python` の順に、Python 3 として起動できるもののうち、
   `-m pytest` が使える最初のもの（どれにも無ければ、起動できる最初のもので `-m unittest <モジュール>`）。Windows の
   Git Bash の `python3` は Microsoft Store のスタブ（何を渡しても失敗する）のことがあり、venv には `python` しか
   無い。`python3` も `python` も PATH に無ければ SKIP、PATH にあってどれも起動できなければ失敗
4. テストが 1 つも無ければ SKIP

3 のテストを手で流すときも、インタープリタを同じく選ぶ（Windows では `python3 -m pytest` でなく `python -m pytest`）。
