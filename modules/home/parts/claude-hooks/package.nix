# claude-hooks: Claude Code の PreToolUse hook（Rust）。
# claude-code.nix から callPackage され、~/.claude/bin/claude-hooks に配置される。
# flake.nix の packages 出力（nix build .#claude-hooks）もここを参照する
{
  lib,
  rustPlatform,
  git,
}:
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
  # ユニットテストをビルド時に実行する。pre_merge_check のテストは一時ディレクトリに git の
  # リポジトリを作るので、git を渡す（gh と hook の入出力を伴う結合テストは .claude/tests/hooks.sh）
  doCheck = true;
  nativeCheckInputs = [ git ];
  meta.mainProgram = "claude-hooks";
}
