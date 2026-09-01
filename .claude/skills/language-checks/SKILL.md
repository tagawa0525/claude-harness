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

## 実行

言語の検出とチェックの実行はスクリプトが行う:

```bash
~/.claude/skills/language-checks/scripts/run-checks.sh
```

- 検出条件: プロジェクトマーカー（`Cargo.toml` / `pyproject.toml` `setup.py` `requirements.txt` /
  `flake.nix`）があるか、その言語のファイルがステージ済み。Markdown はステージ済みの `.md` のみ
- フォーマット → リント → テストの順に実行し、最初に失敗したチェックで止まる
- 失敗時は `FAILED:` に続けて `FIX:`（自動修正コマンド）を出す。ツールがなければ `SKIP:` で続行

## プロジェクト設定の優先

プロジェクトの CLAUDE.md やドキュメントにチェックコマンドが明記されている
場合は**そちらを優先する**。スクリプトのコマンドは既定値であり、たとえば
feature gate のあるクレートでは `--all-features` が付かないと feature 配下の
コードが一切検査されず、チェックが通っても CI で落ちる。

## チェック失敗時の対応

`FAILED:` のコマンドと出力を提示し、`FIX:` があればそれを提案する。
自動修正で直らない違反の読み解きと、環境起因の失敗（NixOS 特有の問題など）は
下記リファレンスのトラブルシューティングを参照する。

## 参照

- [Rust Checks](./references/rust-checks.md)
- [Python Checks](./references/python-checks.md)
- [Nix Checks](./references/nix-checks.md)
- [Markdown Checks](./references/markdown-checks.md)
