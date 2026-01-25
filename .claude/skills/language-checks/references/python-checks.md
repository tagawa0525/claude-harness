# Python Quality Checks

## Overview

Pythonプロジェクトで実行する品質チェックの詳細です。

## 1. Format Check (ruff format)

### コマンド

```bash
ruff format --check .
```

### 目的

- コードが一貫したフォーマットスタイルに従っているか確認
- `--check` フラグにより、ファイルを変更せずにチェックのみ実行
- Black互換のフォーマットを提供

### 成功条件

- 全てのファイルが既にフォーマット済み
- 終了コード 0

### 失敗時の対応

```bash
# 自動修正
ruff format .
```

### エラー例

```
Would reformat: src/app.py
1 file would be reformatted
```

## 2. Lint Check (ruff check)

### コマンド

```bash
ruff check .
```

### 目的

- コードの品質問題、バグの可能性、スタイル違反を検出
- Flake8, isort, pyupgrade などの機能を統合
- 高速な静的解析

### 成功条件

- エラー・警告が0件
- 終了コード 0

### 失敗時の対応

```bash
# 自動修正可能なものを修正
ruff check --fix .

# 特定のルールを無視（慎重に使用）
ruff check --ignore F401 .
```

### よくあるエラー

- `F401`: モジュールがインポートされているが未使用
- `E501`: 行が長すぎる（デフォルト88文字）
- `F841`: ローカル変数が割り当てられているが未使用
- `I001`: インポートの順序が正しくない
- `N806`: 変数名が小文字のスネークケースではない

### エラー例

```
src/app.py:15:1: F401 [*] `os` imported but unused
src/app.py:42:1: E501 Line too long (95 > 88 characters)
src/utils.py:10:5: F841 Local variable `result` is assigned to but never used
Found 3 errors.
[*] 1 fixable with the `--fix` option.
```

## 3. Test (pytest)

### コマンド

```bash
pytest
```

### オプション

```bash
# 詳細出力
pytest -v

# 失敗したテストのみ表示
pytest --tb=short

# カバレッジレポート付き
pytest --cov=src --cov-report=term-missing

# 並列実行
pytest -n auto
```

### 目的

- 全てのテストを実行
- コードの正確性を検証
- リグレッションを防ぐ

### 成功条件

- 全てのテストが成功
- 終了コード 0

### 失敗時の対応

- テスト失敗の詳細を確認
- コードまたはテストを修正
- テストをスキップする場合は `@pytest.mark.skip` を使用

### テストの発見

pytest は以下のパターンでテストを自動発見:
- `test_*.py` または `*_test.py` ファイル
- `Test*` クラス
- `test_*` 関数/メソッド

### エラー例

```
================================ FAILURES =================================
__________________________ test_addition __________________________
src/test_math.py:5: in test_addition
    assert add(2, 2) == 5
E   assert 4 == 5
E    +  where 4 = add(2, 2)

========================= short test summary info =========================
FAILED src/test_math.py::test_addition - assert 4 == 5
========================= 1 failed, 2 passed in 0.12s =========================
```

## ツールのインストール

### ruff

```bash
# pip経由
pip install ruff

# Nix経由（推奨）
nix-env -iA nixpkgs.ruff
```

### pytest

```bash
# pip経由
pip install pytest

# Nix経由（推奨）
nix-env -iA nixpkgs.pytest
```

## 設定ファイル

### pyproject.toml

プロジェクトルートに配置:

```toml
[tool.ruff]
# 行の最大長
line-length = 88

# Python バージョン
target-version = "py311"

# 有効にするルール
select = [
    "E",   # pycodestyle errors
    "W",   # pycodestyle warnings
    "F",   # pyflakes
    "I",   # isort
    "N",   # pep8-naming
]

# 無視するルール
ignore = []

[tool.ruff.format]
# Black互換のフォーマット
quote-style = "double"
indent-style = "space"

[tool.pytest.ini_options]
# テストディレクトリ
testpaths = ["tests"]

# 最小カバレッジ
addopts = "--cov=src --cov-fail-under=80"
```

### ruff.toml

または、専用の設定ファイル:

```toml
line-length = 88
target-version = "py311"

select = ["E", "F", "I", "N"]
ignore = ["E501"]

[format]
quote-style = "double"
```

## CI/CD Integration

GitHub Actions の例:

```yaml
- name: Install dependencies
  run: pip install ruff pytest

- name: Check format
  run: ruff format --check .

- name: Run linter
  run: ruff check .

- name: Run tests
  run: pytest
```

## トラブルシューティング

### "ruff: command not found"

```bash
# Nixでインストール
nix-env -iA nixpkgs.ruff

# または pip でインストール
pip install ruff
```

### "pytest: command not found"

```bash
# Nixでインストール
nix-env -iA nixpkgs.pytest

# または pip でインストール
pip install pytest
```

### テストが見つからない

```bash
# テストディレクトリを明示的に指定
pytest tests/

# 詳細な発見ログを表示
pytest --collect-only
```

### インポートエラー

```bash
# プロジェクトルートをPYTHONPATHに追加
PYTHONPATH=. pytest

# または、開発モードでインストール
pip install -e .
```

## ベストプラクティス

1. **pyproject.toml を使用**: 全ての設定を一箇所に集約
2. **pre-commit フックの活用**: コミット前に自動チェック
3. **CI統合**: GitHub ActionsやGitLab CIに組み込む
4. **カバレッジ測定**: テストカバレッジを80%以上に維持
5. **型ヒントの追加**: mypy と組み合わせて型チェックも実施
6. **自動修正の活用**: `ruff check --fix` で修正可能なものは自動修正

## 追加の推奨ツール

### mypy (型チェック)

```bash
mypy src/
```

### bandit (セキュリティチェック)

```bash
bandit -r src/
```

### interrogate (docstring カバレッジ)

```bash
interrogate -v src/
```

## Ruff vs 他のツール

Ruffは以下のツールを置き換え可能:
- **Flake8**: リント
- **Black**: フォーマット
- **isort**: インポート整理
- **pyupgrade**: 新しいPython構文への変換
- **pydocstyle**: docstring スタイルチェック

Ruffの利点:
- 10-100倍高速
- 単一ツールで複数機能
- Rust実装による高パフォーマンス
