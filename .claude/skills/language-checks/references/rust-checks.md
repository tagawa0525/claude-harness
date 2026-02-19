# Rust Quality Checks

Rustプロジェクトで実行する品質チェック。

## 1. Format Check (cargo fmt)

```bash
cargo fmt --check
```

**目的**: Rust標準フォーマットスタイルに従っているか確認

**成功条件**: 終了コード 0

**失敗時の対応**:

```bash
cargo fmt
```

## 2. Lint Check (cargo clippy)

```bash
cargo clippy --all-targets -- -D warnings
```

**目的**: コード品質問題、バグの可能性、非推奨パターンを検出

**成功条件**: 警告が0件、終了コード 0

**失敗時の対応**: エラーメッセージを確認し手動で修正

### オプション説明

- `--all-targets`: tests, benches, examples を含む全ターゲット
- `-- -D warnings`: 全警告をエラー扱い

### よくある警告

| リント               | 説明                   |
| -------------------- | ---------------------- |
| unused_variables     | 未使用の変数           |
| needless_return      | 不要な return 文       |
| redundant_clone      | 不要な clone()         |
| match_single_binding | 単一パターンの match   |

## 3. Test (cargo test)

```bash
cargo test
```

**目的**: ユニットテスト、統合テスト、doctest を実行

**成功条件**: 全テスト成功、終了コード 0

**失敗時の対応**: テスト失敗の詳細を確認し修正

### テストの種類

| 種類              | 場所                        |
| ----------------- | --------------------------- |
| Unit tests        | `#[cfg(test)]` モジュール内 |
| Integration tests | `tests/` ディレクトリ       |
| Doc tests         | ドキュメントコメント内      |

## クイックリファレンス

| ツール       | チェック                       | 自動修正   |
| ------------ | ------------------------------ | ---------- |
| cargo fmt    | `--check`                      | (引数なし) |
| cargo clippy | `--all-targets -- -D warnings` | -          |
| cargo test   | (引数なし)                     | -          |

## トラブルシューティング

### "cargo fmt not found" / "cargo clippy not found"

```bash
rustup component add rustfmt
rustup component add clippy
```

### テストが遅い

```bash
cargo test -- --test-threads=1  # 並列実行数を制限
cargo test test_name            # 特定テストのみ
```
