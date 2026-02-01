# Rust Quality Checks - 詳細リファレンス

基本情報は [rust-checks.md](./rust-checks.md) を参照。

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

### rustfmt.toml

```toml
max_width = 100
tab_spaces = 4
edition = "2021"
```

### clippy.toml

```toml
allow = ["clippy::too_many_arguments"]
```

## CI/CD Integration

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
cargo test -- --test-threads=1  # 並列実行数を制限
cargo test test_name            # 特定テストのみ
```

## ベストプラクティス

1. コミット前に必ず実行
2. CI/CD パイプラインに組み込む
3. 警告をゼロに保つ
4. 重要な機能にはテストを書く
5. チーム全体で rustfmt を使用
