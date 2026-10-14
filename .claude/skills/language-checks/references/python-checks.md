# Python Quality Checks

Pythonプロジェクトで実行する品質チェック。

## 1. Format Check (ruff format)

```bash
ruff format --check .
```

**目的**: Black互換のフォーマットでコードスタイルを統一

**成功条件**: 終了コード 0

**失敗時の対応**:

```bash
ruff format .
```

## 2. Lint Check (ruff check)

```bash
ruff check .
```

**目的**: コード品質問題、バグの可能性、スタイル違反を検出

**成功条件**: エラー・警告が0件、終了コード 0

**失敗時の対応**:

```bash
ruff check --fix .
```

### よくあるエラー

| コード | 説明                             |
| ------ | -------------------------------- |
| F401   | インポートされているが未使用     |
| E501   | 行が長すぎる（88文字超）         |
| F841   | 変数が割り当てられているが未使用 |
| I001   | インポート順序が不正             |

## 3. Test (pytest)

```bash
pytest
```

**目的**: ユニットテスト、統合テストを実行

**成功条件**: 全テスト成功、終了コード 0

**失敗時の対応**: テスト失敗の詳細を確認し修正

**テストの置き場と走らせ方**（run-checks.sh）: `scripts/run-python-tests.sh` があればそれを優先して走らせる（終了コードで判定。
実行権限が無ければ失敗）。無ければ `tests/` かルート直下の `test_*.py` を `pytest` で走らせる。それも無く、git が追跡している
`test_*.py` / `*_test.py` がサブディレクトリにあれば、そのディレクトリごとに、追跡しているファイルだけを渡して走らせる
（`python3 -m pytest` が使えれば pytest、無ければ `python3 -m unittest <モジュール>`）。テストが 1 つも無いときだけ SKIP（ADR-0036）。

### オプション

```bash
pytest -v                              # 詳細出力
pytest --tb=short                      # 短いトレースバック
pytest --cov=src --cov-report=term-missing  # カバレッジ
pytest -n auto                         # 並列実行
```

### テスト発見パターン

- `test_*.py` または `*_test.py` ファイル
- `Test*` クラス
- `test_*` 関数/メソッド

## クイックリファレンス

| ツール      | チェック    | 自動修正  |
| ----------- | ----------- | --------- |
| ruff format | `--check .` | `.`       |
| ruff check  | `.`         | `--fix .` |
| pytest      | (なし)      | -         |

## トラブルシューティング

### テストが見つからない

```bash
pytest tests/           # ディレクトリ明示
pytest --collect-only   # 発見ログ表示
```

### インポートエラー

```bash
PYTHONPATH=. pytest     # パス追加
pip install -e .        # 開発モード
```
