# Nix Quality Checks

Nixプロジェクトで実行する品質チェック。

## 1. Format Check (nixfmt)

```bash
# git管理下の全 .nix ファイルを対象（*.nix だけではサブディレクトリが漏れる）
# NUL区切り（-z/-0）でスペースを含むパスにも対応
git ls-files -z '*.nix' | xargs -0 -r nixfmt --check
```

**目的**: NixコードがRFC 166公式スタイルに従っているか確認

**成功条件**: 終了コード 0

**失敗時の対応**:

```bash
git ls-files -z '*.nix' | xargs -0 -r nixfmt
```

## 2. Static Analysis (statix)

```bash
statix check
```

**目的**: アンチパターン、非推奨な構文、最適化の余地を検出

**成功条件**: 警告・エラーが0件、終了コード 0

**失敗時の対応**:

```bash
statix fix
```

### よくある警告

| ルール             | 説明                        |
| ------------------ | --------------------------- |
| empty_let_in       | 空の `let ... in` ブロック  |
| deprecated_is_null | `isNull` → `== null` を推奨 |
| useless_parens     | 不要な括弧                  |
| bool_comparison    | `x == true` → `x` を推奨    |

## 3. Flake Check (nix flake check)

```bash
nix flake check
```

**目的**: Flakeの整合性を検証、全outputs の評価テスト

**成功条件**: 全outputs が評価成功、終了コード 0

**失敗時の対応**: エラーメッセージを確認し該当outputを修正

### オプション

```bash
nix flake check --system x86_64-linux  # 特定のsystemのみ
nix flake check --no-build             # 評価のみ（高速）
nix flake check --show-trace           # 詳細ログ
```

## クイックリファレンス

| ツール    | チェック          | 自動修正  |
| --------- | ----------------- | --------- |
| nixfmt    | `--check <files>` | `<files>` |
| statix    | `check`           | `fix`     |
| nix flake | `check`           | -         |

## トラブルシューティング

### "infinite recursion" エラー

```bash
nix flake check --show-trace  # スタックトレース表示
```

## 追加ツール

### deadnix (デッドコード検出)

```bash
nix shell nixpkgs#deadnix -c deadnix
```

### nix-update (固定出力ハッシュの更新)

`Cargo.lock` や `package-lock.json` などの依存を更新したあと、`cargoHash`、`vendorHash`、
`npmDepsHash` のような固定出力ハッシュがずれてビルドが失敗するときに使う。
ハッシュは依存を変えたコミットと同じコミットで更新する（別のコミットにすると、その間のコミットが
ビルドできない）。

```bash
nix run nixpkgs#nix-update -- --flake --version=skip <パッケージの属性名>
```

`--version=skip` はバージョンを変えず、ハッシュだけを更新する。`--flake` は flake の
`packages.<system>.<属性>` を対象にする。ハッシュを手で `sed` で書き換えると、取り出しが空のまま
書き込むなどの誤りが起きる。コマンドの成否は終了コードで確かめ、出力を `tail` などにつながない。
