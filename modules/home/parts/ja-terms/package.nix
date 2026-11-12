# ja-terms: 崩れた日本語の語の辞書（prh.yml）を読み、prh 5.4.4（textlint-rule-prh 6.0.0 が使う版）と同じ意味で文に当てる Rust の crate（ADR-0085）。
# flake.nix の packages 出力（nix build .#ja-terms）と checks がここを参照し、ビルドの中でテストを流す
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "ja-terms";
  version = "0.1.0";
  # 同じディレクトリの textlint の設定と sh（textlint.nix）は除き、それらの変更で crate を作り直さない。
  # prh.yml はテストが読む
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./tests
      ./prh.yml
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;
}
