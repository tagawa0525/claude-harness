# commit-refs: 履歴の書き換えで変わったコミットの番号を、文書の中で付け直す CLI（ADR-0012）。
# git.nix から callPackage され、post-rewrite hook が store のパスで呼び、PATH にも入る。
# flake.nix の packages 出力（nix build .#commit-refs）もここを参照する
{ rustPlatform, git }:
rustPlatform.buildRustPackage {
  pname = "commit-refs";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
  # tests/ は一時的なリポジトリで git を走らせる
  nativeCheckInputs = [ git ];
  meta.mainProgram = "commit-refs";
}
