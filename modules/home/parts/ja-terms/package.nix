# ja-terms: 崩れた日本語の語の辞書（prh.yml）を読み、prh 5.4.4（textlint-rule-prh 6.0.0 が使う版）と同じ意味で文に当てる Rust の crate と
# CLI（ADR-0085）。git-hooks.nix（pre-commit と commit-msg）、claude-hooks のラッパー、claude-code.nix（~/.claude/bin）、
# history-rewrite のテストが callPackage する。flake.nix の packages 出力（nix build .#ja-terms）と checks もここを参照し、
# ビルドの中でテストを流す
{
  lib,
  rustPlatform,
  makeBinaryWrapper,
  git,
}:
rustPlatform.buildRustPackage {
  pname = "ja-terms";
  version = "0.1.0";
  # prh.yml は CLI がビルドに埋め、テストも読む
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
  # ja-terms staged のテストは、一時の git のリポジトリにステージして確かめる
  nativeCheckInputs = [ git ];
  # ja-terms staged は git を呼ぶ。pre-commit が呼ぶ版は、利用者の PATH（git の無い PATH や古い git）に左右されない
  # よう store の git を先に置く（cargo install する Windows では PATH の git を使う）
  nativeBuildInputs = [ makeBinaryWrapper ];
  postInstall = ''
    wrapProgram $out/bin/ja-terms --prefix PATH : ${lib.makeBinPath [ git ]}
  '';
}
