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

| リント               | 説明                 |
| -------------------- | -------------------- |
| unused_variables     | 未使用の変数         |
| needless_return      | 不要な return 文     |
| redundant_clone      | 不要な clone()       |
| match_single_binding | 単一パターンの match |

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

### テストが "memory allocation of N bytes failed" / SIGABRT で落ちる

テストバイナリはデフォルトで CPU 数ぶん並列実行される。メモリ上限のある
環境（サンドボックスの ulimit、コンテナ等）では大きなテストスイートが
確保失敗で abort することがある。コードの問題ではないので、並列度を
下げて再実行する:

```bash
cargo test -- --test-threads=4
```

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

## NixOS環境特有の問題

いずれもコードの問題に見えるが環境起因。プロダクトコード側での回避は禁止。

### cargo / rustc が "No such file or directory (os error 2)" で起動しない

過去に patchelf で ELF インタープリタを nix store パス直指定に書き換えた
バイナリは、システム更新後の GC でそのパスが消えると壊れる。再パッチせず
ツールチェーンを入れ直して未加工バイナリに戻す（rustup 製バイナリは
nix-ld 経由でそのまま動き、GC 耐性がある）:

```bash
# インタープリタが /nix/store/... 直指定なら patchelf 痕（要入れ直し）
file ~/.rustup/toolchains/*/bin/cargo

# 対象のツールチェーン名を確認して入れ直す
rustup toolchain list
rustup toolchain uninstall stable
rustup toolchain install stable

# 追加ターゲットを使っていた場合は入れ直す
rustup target add wasm32-unknown-unknown
```

### `error: command failed: 'cargo-fmt': No such file or directory`

`~/.cargo/bin/cargo-fmt` / `cargo-clippy` が rustup proxy で、その環境で
rustup バイナリが実行不能な場合に全プロジェクトで失敗する。該当 proxy を
削除すると cargo が PATH 上の実バイナリ（devenv / nix のツールチェーン）に
フォールバックして動く。

### bindgen が "Unable to find libclang" で panic する

開発環境に libclang が宣言されていない。恒久対応は devenv.nix / flake.nix に
libclang を追加。暫定なら `LIBCLANG_PATH` を設定してから実行する。

### build script が "could not execute process … (never executed)" で失敗する

古い環境世代でビルドされた `target/` 内の build script の ELF インタープリタが
GC 済み。`cargo clean -p <crate>` で該当クレートのみ再ビルドする
（フルクリーンは大規模プロジェクトでは高コスト）。
