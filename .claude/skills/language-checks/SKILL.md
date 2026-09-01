---
name: language-checks
description: 言語別の品質チェック知識ベース。Rust、Python、Nix、Markdownのフォーマット/リント/テストコマンドを提供。
user-invocable: false
---

# Language Quality Checks Skill

プログラミング言語ごとの品質チェック（フォーマット、リント、テスト）の知識ベース。

## 対応言語

- **Rust**: cargo fmt, clippy, test
- **Python**: ruff format, ruff check, pytest
- **Nix**: nixfmt, statix, nix flake check
- **Markdown**: markdownlint, fix-markdown-lint.py

## 言語検出方法

いずれかの条件を満たす言語のチェックを実行する:

- **Rust**: `Cargo.toml` が存在、または `.rs` ファイルがstaged
- **Python**: `pyproject.toml` / `setup.py` / `requirements.txt` のいずれかが存在、または `.py` ファイルがstaged
- **Nix**: `flake.nix` が存在、または `.nix` ファイルがstaged
- **Markdown**: `.md` ファイルがstaged

## プロジェクト設定の優先

プロジェクトの CLAUDE.md やドキュメントにチェックコマンドが明記されている
場合は**そちらを優先する**。本スキルのコマンドは既定値であり、たとえば
feature gate のあるクレートでは `--all-features` が付かないと feature 配下の
コードが一切検査されず、チェックが通っても CI で落ちる。

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
git ls-files -z '*.nix' | xargs -0 -r nixfmt --check

# 2. 静的解析
statix check

# 3. Flakeの検証（flake.nixが存在する場合）
nix flake check
```

### Markdown

```bash
# 1. 自動修正（markdownlint が直せるもの）
markdownlint --fix <files>

# 2. 自動修正の補完（MD040 言語指定なし / MD060 CJK テーブル整列）
python3 ~/.claude/skills/language-checks/scripts/fix-markdown-lint.py <files>

# 3. 残った違反の確認
markdownlint <files>
```

## チェック失敗時の対応

フォーマット → リント → テストの順に実行し、最初に失敗したチェックで中断する。
失敗したコマンドと出力を提示し、自動修正コマンドがあれば提案する。
各言語の自動修正コマンドは下記リファレンスを参照。

## 参照

- [Rust Checks](./references/rust-checks.md)
- [Python Checks](./references/python-checks.md)
- [Nix Checks](./references/nix-checks.md)
- [Markdown Checks](./references/markdown-checks.md)
