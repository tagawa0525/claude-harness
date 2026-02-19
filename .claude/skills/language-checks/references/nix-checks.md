# Nix Quality Checks

Nixプロジェクトで実行する品質チェック。

## 1. Format Check (nixpkgs-fmt)

```bash
nixpkgs-fmt --check *.nix
```

**目的**: Nixコードが標準的なフォーマットスタイルに従っているか確認

**成功条件**: 終了コード 0

**失敗時の対応**:

```bash
nixpkgs-fmt *.nix
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

| ルール               | 説明                              |
| -------------------- | --------------------------------- |
| empty_let_in         | 空の `let ... in` ブロック        |
| deprecated_is_null   | `isNull` → `== null` を推奨       |
| useless_parens       | 不要な括弧                        |
| bool_comparison      | `x == true` → `x` を推奨          |

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

| ツール      | チェック         | 自動修正 |
| ----------- | ---------------- | -------- |
| nixpkgs-fmt | `--check *.nix`  | `*.nix`  |
| statix      | `check`          | `fix`    |
| nix flake   | `check`          | -        |

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
