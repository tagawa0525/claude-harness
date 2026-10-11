# design-records: 設計の記録（ADR と issue）の雛形を作り、番号を予約して検査する CLI（ADR-0009、ADR-0018）。
# git.nix から callPackage され、PATH に入る（各プロジェクトの pre-commit が呼ぶ）。
# claude-hooks はこのクレートをライブラリとして使う（../claude-hooks/package.nix）。
# flake.nix の packages 出力（nix build .#design-records）もここを参照する
{ rustPlatform, git }:
rustPlatform.buildRustPackage {
  pname = "design-records";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
  # tests/cli.rs は一時的なリポジトリ（bare のリモートを含む）で git を走らせる
  nativeCheckInputs = [ git ];
  meta.mainProgram = "design-records";
}
