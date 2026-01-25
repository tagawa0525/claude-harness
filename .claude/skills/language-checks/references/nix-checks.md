# Nix Quality Checks

## Overview

Nixプロジェクトで実行する品質チェックの詳細です。

## 1. Format Check (nixpkgs-fmt)

### コマンド

```bash
nixpkgs-fmt --check *.nix
```

### 目的

- Nixコードが標準的なフォーマットスタイルに従っているか確認
- `--check` フラグにより、ファイルを変更せずにチェックのみ実行
- Nixpkgs公式のフォーマットスタイルを適用

### 成功条件

- 全ての `.nix` ファイルが既にフォーマット済み
- 終了コード 0

### 失敗時の対応

```bash
# 自動修正
nixpkgs-fmt *.nix

# または、特定のファイルのみ
nixpkgs-fmt flake.nix
```

### エラー例

```
flake.nix
configuration.nix
2 files would be reformatted
```

### フォーマットの特徴

- インデントは2スペース
- 属性セットは縦に整列
- リストは適切に改行
- 一貫した空白の使用

## 2. Static Analysis (statix)

### コマンド

```bash
statix check
```

### 目的

- Nixコードの静的解析
- アンチパターン、非推奨な構文、最適化の余地を検出
- Nixpkgsのベストプラクティスに従っているか確認

### 成功条件

- 警告・エラーが0件
- 終了コード 0

### 失敗時の対応

```bash
# 自動修正可能なものを修正
statix fix

# 特定のファイルのみチェック
statix check flake.nix
```

### よくある警告

- **empty_let_in**: 空の `let ... in` ブロック
- **deprecated_is_null**: `isNull` の代わりに `== null` を使用推奨
- **useless_parens**: 不要な括弧
- **empty_inherit**: 空の `inherit` 文
- **bool_comparison**: `x == true` の代わりに `x` を使用
- **redundant_pattern_bind**: 冗長なパターンバインディング

### エラー例

```
[W04] Warning: Found empty let-in block
   ╭────
 5 │   let
 6 │   in
   ·   ▲
   ·   ╰─ This let-in block is empty
   ╰────
   help: Remove the empty let-in block
```

### Statix のルール

- **W01-W30**: 警告レベル（Warning）
- **E01-E10**: エラーレベル（Error）

主要なルール:
- `empty_let_in`: 空のlet-inブロック
- `deprecated_*`: 非推奨な関数の使用
- `useless_*`: 不要なコード
- `bool_comparison`: 冗長なbool比較

## 3. Flake Check (nix flake check)

### コマンド

```bash
nix flake check
```

### 目的

- Flakeの整合性を検証
- 全ての outputs が正しく評価できるか確認
- パッケージ、devShells、nixosConfigurations などをテスト

### 成功条件

- 全ての outputs が評価成功
- テストが全て成功
- 終了コード 0

### 失敗時の対応

- エラーメッセージを確認
- 該当する output を修正
- 依存関係の問題を解決

### チェック内容

1. **Flake metadata**: `flake.nix` の構文チェック
2. **Outputs evaluation**: 全ての outputs が評価可能か
3. **Packages**: パッケージがビルド可能か
4. **Checks**: 定義された checks を実行
5. **NixOS configurations**: システム設定が評価可能か

### エラー例

```
error: attribute 'packages.x86_64-linux.mypackage' is not a derivation
       at /nix/store/...-source/flake.nix:25:7:
           24|       packages = {
           25|         mypackage = "not a derivation";
             |         ^
           26|       };
```

### オプション

```bash
# 特定の system のみチェック
nix flake check --system x86_64-linux

# ビルドせずに評価のみ
nix flake check --no-build

# 詳細ログ
nix flake check --show-trace
```

## ツールのインストール

### nixpkgs-fmt

```bash
# Nix経由
nix-env -iA nixpkgs.nixpkgs-fmt

# または、nix shell で一時的に使用
nix shell nixpkgs#nixpkgs-fmt
```

### statix

```bash
# Nix経由
nix-env -iA nixpkgs.statix

# または、nix shell で一時的に使用
nix shell nixpkgs#statix
```

## 設定ファイル

### .statix.toml

プロジェクトルートに配置してstatixの動作をカスタマイズ:

```toml
# 無視するディレクトリ
ignore = [
    ".git",
    "result",
    "target",
]

# 無効にするルール
disabled = []

# フォーマットの設定
[format]
# インデント幅
indent_width = 2
```

### .editorconfig

エディタのフォーマット設定:

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

### "nixpkgs-fmt: command not found"

```bash
# インストール
nix-env -iA nixpkgs.nixpkgs-fmt

# または、一時的に使用
nix shell nixpkgs#nixpkgs-fmt -c nixpkgs-fmt --check *.nix
```

### "statix: command not found"

```bash
# インストール
nix-env -iA nixpkgs.statix

# または、一時的に使用
nix shell nixpkgs#statix -c statix check
```

### "nix flake check" が遅い

```bash
# ビルドをスキップして評価のみ
nix flake check --no-build

# 特定のsystemのみチェック
nix flake check --system x86_64-linux
```

### "infinite recursion" エラー

```bash
# スタックトレースを表示
nix flake check --show-trace

# 該当箇所を特定して循環参照を解消
```

## ベストプラクティス

1. **フォーマットの統一**: 全ての `.nix` ファイルに nixpkgs-fmt を適用
2. **statix の定期実行**: コミット前に必ずチェック
3. **flake.lock のコミット**: lockファイルは必ずバージョン管理
4. **CI統合**: GitHub ActionsでFlakeの整合性を検証
5. **評価の最適化**: `nix flake check --no-build` で高速化
6. **明確なエラーメッセージ**: `assert` や `throw` でわかりやすいエラーを提供

## 追加の推奨ツール

### deadnix (デッドコード検出)

```bash
# インストール
nix shell nixpkgs#deadnix

# 未使用のコードを検出
deadnix
```

### nix-linter (追加のリント)

```bash
# インストール
nix shell nixpkgs#nix-linter

# リント実行
nix-linter *.nix
```

### alejandra (代替フォーマッタ)

```bash
# インストール
nix shell nixpkgs#alejandra

# フォーマット
alejandra .
```

## nixpkgs-fmt vs alejandra

### nixpkgs-fmt
- Nixpkgs公式のフォーマッタ
- 保守的なフォーマット
- Nixpkgsプロジェクトで使用

### alejandra
- より現代的なフォーマット
- 積極的な改行とインデント
- 個人プロジェクトで人気

**推奨**: nixpkgs-fmt（Nixpkgs標準に準拠）

## Flake Checks の例

`flake.nix` でカスタムチェックを定義:

```nix
{
  outputs = { self, nixpkgs }: {
    checks.x86_64-linux = {
      # フォーマットチェック
      format-check = nixpkgs.legacyPackages.x86_64-linux.runCommand "format-check" {
        buildInputs = [ nixpkgs.legacyPackages.x86_64-linux.nixpkgs-fmt ];
      } ''
        nixpkgs-fmt --check ${self}
        touch $out
      '';

      # Statix チェック
      statix-check = nixpkgs.legacyPackages.x86_64-linux.runCommand "statix-check" {
        buildInputs = [ nixpkgs.legacyPackages.x86_64-linux.statix ];
      } ''
        statix check ${self}
        touch $out
      '';
    };
  };
}
```

これにより `nix flake check` で自動的にフォーマットとlintがチェックされます。
