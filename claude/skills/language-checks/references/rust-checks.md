# Rust Quality Checks

上から順に実行する。どれも終了コード 0 で成功。

## 1. Format Check (cargo fmt)

```bash
cargo fmt --check   # 検査
cargo fmt           # 失敗時の自動修正
```

## 2. Lint Check (cargo clippy)

```bash
cargo clippy --all-targets -- -D warnings
```

自動修正は無い。メッセージを確認して手で直す。

## 3. Test (cargo test)

```bash
cargo test
```

**文書だけの差分**（run-checks.sh）:

- ルートに `Cargo.toml` と `scripts/run-rust-docs-tests.sh` があり、差分が空でなくすべて `.md`（削除と rename の
  前後を含む）なら、fmt、clippy、全体の `cargo test` の代わりにそのスクリプトだけを実行する
- `.md` 以外（`clippy.toml` やテストが読むデータを含む）を含む差分では全体を実行し、その後にスクリプトも実行する
- スクリプトは `bash <スクリプト>` で実行し、終了コードで判定する。実行権限が無ければ差分によらず失敗する
- スクリプトが無いリポジトリは、差分によらず全体を実行する

スクリプトは、文書を読むテストを持つリポジトリが書く。中身は文書の差分で実行するテスト
（例: `exec cargo test -p <文書のテストの crate>`）。`.md` を `include_str!` で取り込むクレート（doctest や clippy の
doc の lint が `.md` で変わる）と、実行時に `.md` を読むテスト（fixture など）もスクリプトに含める。含められなければ
スクリプトを置かない。

## トラブルシューティング

### テストが "memory allocation of N bytes failed" / SIGABRT で落ちる

テストは既定で CPU 数ぶん並列に走るので、メモリ上限のある環境（サンドボックスの ulimit、コンテナなど）では
確保に失敗する。コードの問題ではないので、並列度を下げて再実行する:

```bash
cargo test -- --test-threads=4
```

## NixOS環境特有の問題

いずれもコードの問題に見えるが環境起因。環境の側で直し、プロダクトコードは変えない。

### cargo / rustc が "No such file or directory (os error 2)" で起動しない

patchelf で ELF インタープリタを nix store のパスに書き換えたバイナリは、GC でそのパスが消えると壊れる。
再パッチせず、ツールチェーンを入れ直して未加工のバイナリに戻す（rustup のバイナリは nix-ld 経由でそのまま動く）:

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

`~/.cargo/bin/cargo-fmt` / `cargo-clippy` が rustup proxy で、rustup のバイナリが実行できない環境。その proxy を
削除すると、cargo が PATH 上の実バイナリ（devenv / nix のツールチェーン）を使う。

### bindgen が "Unable to find libclang" で panic する

恒久対応は devenv.nix / flake.nix に libclang を追加する。暫定なら `LIBCLANG_PATH` を設定して実行する。

### build script が "could not execute process … (never executed)" で失敗する

`target/` の中の build script の ELF インタープリタが GC 済み。`cargo clean -p <crate>` で該当クレートだけを
再ビルドする（フルクリーンは大きなプロジェクトでは高コスト）。
