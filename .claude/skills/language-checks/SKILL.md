---
name: language-checks
description: |
  プログラミング言語別の品質チェック知識ベース。Rust、Python、Nixプロジェクトの
  フォーマット、リント、テストコマンドを提供。コミット前チェックで自動参照される。
user-invocable: false
---

# Language Quality Checks Skill

プログラミング言語ごとの品質チェック（フォーマット、リント、テスト）の知識ベース。

## 対応言語

- **Rust**: cargo fmt, clippy, test
- **Python**: ruff format, ruff check, pytest
- **Nix**: nixpkgs-fmt, statix, nix flake check

## 言語検出方法

### Rustプロジェクト

**検出条件**:

- `Cargo.toml` が存在する
- または `.rs` ファイルがstaged

**プロジェクトマーカー**: `Cargo.toml`

### Pythonプロジェクト

**検出条件**:

- `pyproject.toml`, `setup.py`, `requirements.txt` のいずれかが存在する
- または `.py` ファイルがstaged

**プロジェクトマーカー**: `pyproject.toml`, `setup.py`, `requirements.txt`

### Nixプロジェクト

**検出条件**:

- `flake.nix` が存在する
- または `.nix` ファイルがstaged

**プロジェクトマーカー**: `flake.nix`

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
# 1. フォーマットチェック
nixpkgs-fmt --check *.nix

# 2. 静的解析
statix check

# 3. Flakeの検証（flake.nixが存在する場合）
nix flake check
```

## エラーハンドリング

### チェック失敗時の動作

1. **即座に中断**: いずれかのチェックが失敗した場合、後続のチェックは実行せずに中断
2. **詳細なエラー表示**: 失敗したコマンドと出力を表示
3. **修正方法の提案**: 可能な場合、自動修正コマンドを提案

### 例: Rustフォーマットエラー

```text
❌ Rust format check failed:
   Command: cargo fmt --check

   Error output:
   Diff in src/main.rs at line 42

   💡 Fix: Run 'cargo fmt' to auto-format
```

### 例: Pythonリントエラー

```text
❌ Python lint check failed:
   Command: ruff check .

   Error output:
   src/app.py:15:1: F401 [*] `os` imported but unused

   💡 Fix: Run 'ruff check --fix .' to auto-fix
```

## ベストプラクティス

1. **段階的チェック**: フォーマット → リント → テストの順で実行
2. **早期失敗**: 最初の失敗で中断し、修正を促す
3. **明確なフィードバック**: どのチェックが失敗したか、どう修正するかを明示
4. **スコープの限定**: staged filesに関連するチェックのみ実行

## ツールの可用性確認

チェック実行前に、必要なツールがインストールされているか確認:

```bash
# Rust
command -v cargo >/dev/null 2>&1 || echo "cargo not found"

# Python
command -v ruff >/dev/null 2>&1 || echo "ruff not found"
command -v pytest >/dev/null 2>&1 || echo "pytest not found"

# Nix
command -v nixpkgs-fmt >/dev/null 2>&1 || echo "nixpkgs-fmt not found"
command -v statix >/dev/null 2>&1 || echo "statix not found"
```

## 参照

詳細なチェック項目については、以下を参照:

- [Rust Checks](./references/rust-checks.md)
- [Python Checks](./references/python-checks.md)
- [Nix Checks](./references/nix-checks.md)

## ユーザーへの質問

選択肢を提示する場合は `AskUserQuestion` ツールを使用する。

- 2-4択の明確な選択肢がある場合に使用
- 自由入力が必要な場合（ブランチ名など）は通常のテキスト質問
