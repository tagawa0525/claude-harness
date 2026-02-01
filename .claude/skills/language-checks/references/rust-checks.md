# Rust Quality Checks

## Overview

Rustプロジェクトで実行する品質チェックの詳細です。

## 1. Format Check (cargo fmt)

### コマンド (cargo fmt)

```bash
cargo fmt --check
```

### 目的 (cargo fmt)

- コードが Rust の標準フォーマットスタイルに従っているか確認
- `--check` フラグにより、ファイルを変更せずにチェックのみ実行

### 成功条件 (cargo fmt)

- 全てのファイルが既にフォーマット済み
- 終了コード 0

### 失敗時の対応 (cargo fmt)

```bash
# 自動修正
cargo fmt
```

### エラー例 (cargo fmt)

```text
Diff in src/main.rs at line 42
Diff in src/lib.rs at line 15
```

## 2. Lint Check (cargo clippy)

### コマンド (cargo clippy)

```bash
cargo clippy --all-targets -- -D warnings
```

### オプション説明 (cargo clippy)

- `--all-targets`: tests, benches, examples を含む全ターゲットをチェック
- `-- -D warnings`: 全ての警告をエラーとして扱う

### 目的 (cargo clippy)

- コードの品質問題、バグの可能性、非推奨パターンを検出
- Rustのベストプラクティスに従っているか確認

### 成功条件 (cargo clippy)

- 警告が0件
- 終了コード 0

### 失敗時の対応 (cargo clippy)

- エラーメッセージを確認し、手動で修正
- 一部の警告は `#[allow(clippy::lint_name)]` で抑制可能（ただし慎重に）

### よくある警告

- `unused_variables`: 未使用の変数
- `needless_return`: 不要な return 文
- `redundant_clone`: 不要な clone()
- `match_single_binding`: 単一パターンの match

### エラー例 (cargo clippy)

```text
warning: unused variable: `x`
 --> src/main.rs:5:9
  |
5 |     let x = 42;
  |         ^ help: if this is intentional, prefix it with an underscore: `_x`
  |
  = note: `#[warn(unused_variables)]` on by default
```

## 3. Test (cargo test)

### コマンド (cargo test)

```bash
cargo test
```

### 目的 (cargo test)

- 全てのユニットテスト、統合テスト、doctestを実行
- コードの正確性を検証

### 成功条件 (cargo test)

- 全てのテストが成功
- 終了コード 0

### 失敗時の対応 (cargo test)

- テスト失敗の詳細を確認
- コードまたはテストを修正
- テストが意図的に失敗している場合は、`#[ignore]` を使用

### テストの種類

1. **Unit tests**: `#[cfg(test)]` モジュール内
2. **Integration tests**: `tests/` ディレクトリ内
3. **Doc tests**: ドキュメントコメント内の例

### エラー例 (cargo test)

```text
running 3 tests
test tests::it_works ... ok
test tests::it_fails ... FAILED
test tests::another ... ok

failures:

---- tests::it_fails stdout ----
thread 'tests::it_fails' panicked at 'assertion failed: `(left == right)`
  left: `2`,
 right: `3`', src/lib.rs:42:9
```

## ツールのインストール

### rustfmt

```bash
rustup component add rustfmt
```

### clippy

```bash
rustup component add clippy
```

## 設定ファイル

### rustfmt.toml / .rustfmt.toml

プロジェクトルートに配置してフォーマットルールをカスタマイズ:

```toml
max_width = 100
tab_spaces = 4
edition = "2021"
```

### clippy.toml / .clippy.toml

プロジェクトルートに配置してclippyの動作をカスタマイズ:

```toml
# 特定のリントを許可
allow = ["clippy::too_many_arguments"]
```

## CI/CD Integration

GitHub Actions の例:

```yaml
- name: Check format
  run: cargo fmt --check

- name: Run clippy
  run: cargo clippy --all-targets -- -D warnings

- name: Run tests
  run: cargo test
```

## トラブルシューティング

### "cargo fmt not found"

```bash
rustup component add rustfmt
```

### "cargo clippy not found"

```bash
rustup component add clippy
```

### テストが遅い

```bash
# 並列実行数を制限
cargo test -- --test-threads=1

# 特定のテストのみ実行
cargo test test_name
```

## ベストプラクティス

1. **定期的な実行**: コミット前に必ず実行
2. **CI統合**: CI/CDパイプラインに組み込む
3. **警告ゼロ**: 警告を放置せず、すぐに対処
4. **テストカバレッジ**: 重要な機能は必ずテストを書く
5. **フォーマット統一**: チーム全体で rustfmt を使用
