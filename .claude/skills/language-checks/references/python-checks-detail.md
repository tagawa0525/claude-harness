# Python Quality Checks - 詳細リファレンス

基本情報は [python-checks.md](./python-checks.md) を参照。

## ツールのインストール

### ruff

```bash
pip install ruff
# または Nix経由（推奨）
nix-env -iA nixpkgs.ruff
```

### pytest

```bash
pip install pytest
# または Nix経由（推奨）
nix-env -iA nixpkgs.pytest
```

## 設定ファイル

### pyproject.toml

```toml
[tool.ruff]
line-length = 88
target-version = "py311"
select = ["E", "W", "F", "I", "N"]
ignore = []

[tool.ruff.format]
quote-style = "double"
indent-style = "space"

[tool.pytest.ini_options]
testpaths = ["tests"]
addopts = "--cov=src --cov-fail-under=80"
```

### ruff.toml

```toml
line-length = 88
target-version = "py311"
select = ["E", "F", "I", "N"]
ignore = ["E501"]

[format]
quote-style = "double"
```

## CI/CD Integration

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

### "command not found"

```bash
# ruff
nix-env -iA nixpkgs.ruff
# または
pip install ruff

# pytest
nix-env -iA nixpkgs.pytest
# または
pip install pytest
```

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

## ベストプラクティス

1. pyproject.toml に設定を集約
2. pre-commit フックで自動チェック
3. CI に組み込む
4. カバレッジ 80% 以上を維持
5. mypy で型チェック追加

## 追加ツール

### mypy (型チェック)

```bash
mypy src/
```

### bandit (セキュリティ)

```bash
bandit -r src/
```

## Ruff が置き換えるツール

| 旧ツール  | 機能           |
| --------- | -------------- |
| Flake8    | リント         |
| Black     | フォーマット   |
| isort     | インポート整理 |
| pyupgrade | 構文更新       |

Ruff は 10-100倍高速（Rust実装）
