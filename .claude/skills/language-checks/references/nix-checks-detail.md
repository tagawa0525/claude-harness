# Nix Quality Checks - 詳細リファレンス

基本情報は [nix-checks.md](./nix-checks.md) を参照。

## ツールのインストール

### nixpkgs-fmt

```bash
nix-env -iA nixpkgs.nixpkgs-fmt
# または一時的に使用
nix shell nixpkgs#nixpkgs-fmt
```

### statix

```bash
nix-env -iA nixpkgs.statix
# または一時的に使用
nix shell nixpkgs#statix
```

## 設定ファイル

### .statix.toml

```toml
ignore = [".git", "result", "target"]
disabled = []

[format]
indent_width = 2
```

### .editorconfig

```ini
[*.nix]
indent_style = space
indent_size = 2
end_of_line = lf
charset = utf-8
trim_trailing_whitespace = true
insert_final_newline = true
```

## CI/CD Integration

GitHub Actions の例:

```yaml
- name: Install Nix
  uses: cachix/install-nix-action@v20
  with:
    nix_path: nixpkgs=channel:nixos-unstable

- name: Check format
  run: nix shell nixpkgs#nixpkgs-fmt -c nixpkgs-fmt --check *.nix

- name: Run statix
  run: nix shell nixpkgs#statix -c statix check

- name: Check flake
  run: nix flake check
```

## トラブルシューティング

### "command not found"

```bash
# nixpkgs-fmt
nix shell nixpkgs#nixpkgs-fmt -c nixpkgs-fmt --check *.nix

# statix
nix shell nixpkgs#statix -c statix check
```

### "nix flake check" が遅い

```bash
nix flake check --no-build           # ビルドスキップ
nix flake check --system x86_64-linux  # 特定systemのみ
```

### "infinite recursion" エラー

```bash
nix flake check --show-trace  # スタックトレース表示
```

## ベストプラクティス

1. 全 `.nix` ファイルに nixpkgs-fmt を適用
2. コミット前に statix チェック
3. flake.lock をバージョン管理
4. CI で Flake の整合性を検証
5. `--no-build` で高速化

## 追加ツール

### deadnix (デッドコード検出)

```bash
nix shell nixpkgs#deadnix -c deadnix
```

### alejandra (代替フォーマッタ)

```bash
nix shell nixpkgs#alejandra -c alejandra .
```

## nixpkgs-fmt vs alejandra

| 項目 | nixpkgs-fmt    | alejandra          |
| ---- | -------------- | ------------------ |
| 特徴 | 公式、保守的   | 現代的、積極的     |
| 用途 | Nixpkgs準拠    | 個人プロジェクト   |

**推奨**: nixpkgs-fmt（Nixpkgs標準に準拠）

## Flake Checks の例

```nix
{
  outputs = { self, nixpkgs }: {
    checks.x86_64-linux = {
      format-check =
        nixpkgs.legacyPackages.x86_64-linux.runCommand "format-check" {
          buildInputs =
            [ nixpkgs.legacyPackages.x86_64-linux.nixpkgs-fmt ];
        } ''
          nixpkgs-fmt --check ${self}
          touch $out
        '';
    };
  };
}
```
