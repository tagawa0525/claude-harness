# Nix Quality Checks

上から順に実行する。どれも終了コード 0 で成功。

## 1. Format Check (nixfmt)

```bash
git ls-files -z '*.nix' | xargs -0 -r nixfmt --check   # 検査
git ls-files -z '*.nix' | xargs -0 -r nixfmt           # 失敗時の自動修正
```

git 管理下の全 `.nix` を渡す（`*.nix` だけではサブディレクトリが漏れる）。

## 2. Static Analysis (statix)

```bash
statix check   # 検査
statix fix     # 失敗時の自動修正
```

## 3. Flake Check (nix flake check)

```bash
nix flake check
```

自動修正は無い。エラーの出た output を直す。

## nix-update (固定出力ハッシュの更新)

`Cargo.lock` や `package-lock.json` などの依存を更新して、`cargoHash`、`vendorHash`、`npmDepsHash` のような
固定出力ハッシュがずれてビルドが失敗するときに使う。

```bash
nix run nixpkgs#nix-update -- --flake --version=skip <パッケージの属性名>
```

- `--version=skip` はバージョンを変えずハッシュだけを更新し、`--flake` は flake の `packages.<system>.<属性>` を対象にする
- ハッシュは、依存を変えたコミットと同じコミットで更新する（分けると、間のコミットがビルドできない）
- ハッシュはこのコマンドで書き換える（手の `sed` は、取り出しが空のまま書き込むなどの誤りを生む）。成否は終了コードで確かめる
