---
name: language-checks
description: 言語別の品質チェック知識ベース。Rust、Python、Nixのフォーマット/リント/テストコマンドを提供。
user-invocable: false
---

# Language Quality Checks Skill

プログラミング言語ごとの品質チェック（フォーマット、リント、テスト）の知識ベース。

## 対応言語

- **Rust**: cargo fmt, clippy, test
- **Python**: ruff format, ruff check, pytest
- **Nix**: nixpkgs-fmt, statix, nix flake check

## 言語検出方法

いずれかの条件を満たす言語のチェックを実行する:

- **Rust**: `Cargo.toml` が存在、または `.rs` ファイルがstaged
- **Python**: `pyproject.toml` / `setup.py` / `requirements.txt` のいずれかが存在、または `.py` ファイルがstaged
- **Nix**: `flake.nix` が存在、または `.nix` ファイルがstaged

## チェックコマンド

### Rust

```bash
# 1. フォーマットチェック（変更を加えない）
cargo fmt --check

# 2. リント（全警告をエラー扱い）
cargo clippy --all-targets -- -D warnings

# 3. テスト実行
cargo test
```

### Python

```bash
# 1. フォーマットチェック
ruff format --check .

# 2. リント
ruff check .

# 3. テスト実行（テストディレクトリが存在する場合）
pytest
```

### Nix

```bash
# 1. フォーマットチェック（サブディレクトリ含む全 .nix ファイル）
git ls-files -z '*.nix' | xargs -0 -r nixpkgs-fmt --check

# 2. 静的解析
statix check

# 3. Flakeの検証（flake.nixが存在する場合）
nix flake check
```

## チェック失敗時の対応

フォーマット → リント → テストの順に実行し、最初に失敗したチェックで中断する。
失敗したコマンドと出力を提示し、自動修正コマンドがあれば提案する。
各言語の自動修正コマンドは下記リファレンスを参照。

## 参照

- [Rust Checks](./references/rust-checks.md)
- [Python Checks](./references/python-checks.md)
- [Nix Checks](./references/nix-checks.md)
