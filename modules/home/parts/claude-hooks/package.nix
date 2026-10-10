# claude-hooks: Claude Code の PreToolUse hook（Rust）。
# claude-code.nix から callPackage され、~/.claude/bin/claude-hooks に配置される。
# flake.nix の packages 出力（nix build .#claude-hooks）もここを参照する
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "claude-hooks";
  version = "0.1.0";
  # パス依存の ../design-records（ADR の形）もソースに入れ、claude-hooks の中でビルドする
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ./.
      ../design-records
    ];
  };
  buildAndTestSubdir = "claude-hooks";
  cargoRoot = "claude-hooks";
  cargoLock.lockFile = ./Cargo.lock;
  # 構文解析と引数の読み取りのユニットテストをビルド時に実行する
  # （git / gh の実行を伴う結合テストは .claude/tests/hooks.sh）
  doCheck = true;
  meta.mainProgram = "claude-hooks";
}
