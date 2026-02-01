# Python Quality Checks

Pythonプロジェクトで実行する品質チェック。

詳細情報は [python-checks-detail.md](./python-checks-detail.md) を参照。

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

| ツール      | チェック     | 自動修正  |
| ----------- | ------------ | --------- |
| ruff format | `--check .`  | `.`       |
| ruff check  | `.`          | `--fix .` |
| pytest      | (なし)       | -         |
