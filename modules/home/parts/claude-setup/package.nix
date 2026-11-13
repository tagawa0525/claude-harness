# claude-setup: ハーネスを Claude Code の設定に配る設置のコマンド（ADR-0084）。claude-code.nix から callPackage され、
# claude-sync と home-manager の activation が呼ぶ。flake.nix の packages 出力（nix build .#claude-setup）と checks も
# ここを参照する
{
  rustPlatform,
  git,
  callPackage,
}:
let
  gitCheckConfig = callPackage ../lib/git-check-config.nix { };
in
rustPlatform.buildRustPackage {
  pname = "claude-setup";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
  # tests/sync.rs は一時の clone（bare の origin を含む）で git を実行する
  nativeCheckInputs = [ git ];
  # sandbox に無い git の全体設定を環境の側から渡す（lib/git-check-config.nix）
  inherit (gitCheckConfig) preCheck;
  meta.mainProgram = "claude-setup";
}
